//! Persona registry bridge between TauriTavern PNG cards and SillyTavern settings.
//!
//! TauriTavern stores each persona's name and descriptor inside its avatar
//! image (a `persona` keyword `iTXt` chunk in `User Avatars/<id>.png`) and
//! keeps `power_user.personas` / `power_user.persona_descriptions` out of
//! `settings.json` (it migrates them into the cards on startup). SillyTavern
//! stores the same registry only in `settings.json` and never reads PNG text
//! chunks. This module provides both projections so the settings translation
//! layer can keep the two representations in lockstep:
//!
//! - TauriTavern -> SillyTavern (push): avatar cards land on disk, then the
//!   registry is projected back into `settings.json` (otherwise SillyTavern's
//!   `addMissingPersonas` repopulates empty `[Unnamed Persona]` shells).
//! - SillyTavern -> TauriTavern (pull): the registry is stripped from the
//!   settings core view and injected into the avatar bytes, so the client
//!   receives persona data through the image channel and never sees a
//!   `personas` key that would trigger its card migration.
//!
//! The JSON shape and the take/insert semantics are ported verbatim from
//! TauriTavern's `tt-domain/src/models/persona.rs`; the chunk layout matches
//! `tt-adapter-storage-core/src/png_metadata.rs` and the card keyword/format
//! in `tt-adapter-media/src/persona_cards.rs` (uncompressed `iTXt`, compact
//! JSON, card injected before `IEND`, all existing `persona` text chunks
//! removed). PNG parsing is dependency-free chunk walking: pixel data is
//! never decoded.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Wire path prefix of the persona avatar directory (dataset
/// `character.avatars`).
pub(crate) const AVATARS_WIRE_PREFIX: &str = "default-user/User Avatars/";

const PNG_SIGNATURE: &[u8] = &[137, 80, 78, 71, 13, 10, 26, 10];
const PERSONA_KEYWORD: &str = "persona";

/// Placeholder name SillyTavern's `addMissingPersonas()` writes when it finds
/// an avatar file without a registry entry
/// (`initPersona(id, '[Unnamed Persona]', '', ...)`). It is a "data missing"
/// sentinel rather than a real name, so the bridge treats entries carrying it
/// as absent: they are never injected into avatar bytes (that would overwrite
/// a good TauriTavern card with the shell), and any on-disk card wins over
/// them regardless of mtime (self-healing `settings.json` without a manual
/// edit on the SillyTavern side).
const UNNAMED_PERSONA_SHELL: &str = "[Unnamed Persona]";

pub(crate) fn is_unnamed_shell(card: &PersonaCard) -> bool {
    card.name.as_deref() == Some(UNNAMED_PERSONA_SHELL)
}

/// One persona entry, exactly the JSON object TauriTavern embeds in a card.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PersonaCard {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<Value>,
}

impl PersonaCard {
    #[cfg(test)]
    pub(crate) fn new(name: Option<String>, description: Option<Value>) -> Self {
        Self { name, description }
    }
}

pub(crate) type PersonaRegistry = BTreeMap<String, PersonaCard>;

/// Read the persona registry from `settings.power_user` without mutating it.
pub(crate) fn read_persona_registry(settings: &Value) -> Result<PersonaRegistry, String> {
    let Some(power_user) = settings.get("power_user") else {
        return Ok(PersonaRegistry::new());
    };
    let mut personas = PersonaRegistry::new();
    if let Some(names) = power_user.get("personas") {
        let Some(names) = names.as_object() else {
            return Err("Invalid Persona names".into());
        };
        for (id, value) in names {
            let Some(name) = value.as_str() else {
                return Err("Invalid Persona names".into());
            };
            personas.entry(id.clone()).or_default().name = Some(name.to_owned());
        }
    }
    if let Some(descriptions) = power_user.get("persona_descriptions") {
        let Some(descriptions) = descriptions.as_object() else {
            return Err("Invalid Persona descriptions".into());
        };
        for (id, value) in descriptions {
            personas.entry(id.clone()).or_default().description = Some(value.clone());
        }
    }
    Ok(personas)
}

/// Remove the persona registry from `settings.power_user`, returning it.
/// Ported from TauriTavern's `take_personas`.
pub(crate) fn take_persona_registry(settings: &mut Value) -> Result<PersonaRegistry, String> {
    let registry = read_persona_registry(settings)?;
    if let Some(power_user) = settings
        .get_mut("power_user")
        .and_then(Value::as_object_mut)
    {
        power_user.remove("personas");
        power_user.remove("persona_descriptions");
    }
    Ok(registry)
}

/// Write the persona registry back into `settings.power_user`. Ported from
/// TauriTavern's `insert_personas`.
pub(crate) fn insert_persona_registry(settings: &mut Value, personas: &PersonaRegistry) {
    if !settings["power_user"].is_object() {
        settings["power_user"] = serde_json::json!({});
    }
    let mut names = serde_json::Map::new();
    let mut descriptions = serde_json::Map::new();
    for (id, persona) in personas {
        if let Some(name) = &persona.name {
            names.insert(id.clone(), Value::String(name.clone()));
        }
        if let Some(description) = &persona.description {
            descriptions.insert(id.clone(), description.clone());
        }
    }
    // An empty registry means "no personas at all": drop the keys instead of
    // writing empty maps, so a workspace that never had personas does not
    // acquire them as a side effect of a settings merge.
    let power_user = settings["power_user"]
        .as_object_mut()
        .expect("power_user ensured to be an object");
    if names.is_empty() {
        power_user.remove("personas");
    } else {
        power_user.insert("personas".to_owned(), Value::Object(names));
    }
    if descriptions.is_empty() {
        power_user.remove("persona_descriptions");
    } else {
        power_user.insert(
            "persona_descriptions".to_owned(),
            Value::Object(descriptions),
        );
    }
}

/// One avatar file as observed on disk: its id, embedded card (if any) and
/// modification time in milliseconds.
pub(crate) struct OnDiskPersona {
    pub(crate) id: String,
    pub(crate) card: Option<PersonaCard>,
    pub(crate) modified_ms: u64,
}

/// Reconcile the settings-side registry with the on-disk avatar cards.
///
/// Rules:
/// - `[Unnamed Persona]` shells in settings count as missing data, not values.
/// - A settings entry whose avatar file no longer exists is dropped (deletion
///   follows the image).
/// - A card for an id missing from settings (or holding a shell) is adopted,
///   so SillyTavern never keeps an `[Unnamed Persona]` once a real card
///   exists.
/// - When both sides hold real data for an id, the newer write wins: the card
///   wins when the avatar file is at least as new as `settings.json` (an edit
///   made in TauriTavern), otherwise the settings value is kept (an edit made
///   in SillyTavern). A tie favors the card, as TauriTavern is the native
///   owner.
pub(crate) fn reconcile_persona_registry(
    settings_registry: &PersonaRegistry,
    on_disk: impl IntoIterator<Item = OnDiskPersona>,
    settings_modified_ms: u64,
) -> PersonaRegistry {
    let on_disk: Vec<OnDiskPersona> = on_disk.into_iter().collect();
    let mut result = PersonaRegistry::new();

    // Keep real settings entries only while the avatar file still exists.
    // Shells are skipped so the on-disk pass can adopt the real card below.
    for (id, card) in settings_registry {
        if is_unnamed_shell(card) {
            continue;
        }
        if on_disk.iter().any(|item| &item.id == id) {
            result.insert(id.clone(), card.clone());
        }
    }

    // Adopt new cards; overlay cards over settings when the image is newer.
    for item in on_disk {
        let Some(card) = item.card else {
            // File exists without a readable card: settings stays authoritative
            // (e.g. an avatar created directly in SillyTavern).
            continue;
        };
        let overlay = match result.get(&item.id) {
            None => true,
            Some(_) if item.modified_ms >= settings_modified_ms => true,
            Some(_) => false,
        };
        if overlay {
            result.insert(item.id, card);
        }
    }
    result
}

pub(crate) fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(PNG_SIGNATURE)
}

/// Extract the first `persona` text chunk from a PNG. Returns `Ok(None)` for
/// non-PNG bytes or a PNG without a persona chunk.
pub(crate) fn extract_persona(bytes: &[u8]) -> Result<Option<PersonaCard>, String> {
    if !is_png(bytes) {
        return Ok(None);
    }
    for chunk in iter_chunks(bytes)? {
        let Some((keyword, text)) = decode_text_chunk(&chunk) else {
            continue;
        };
        if keyword.eq_ignore_ascii_case(PERSONA_KEYWORD) {
            let card = serde_json::from_str::<PersonaCard>(&text)
                .map_err(|e| format!("Invalid Persona metadata: {e}"))?;
            return Ok(Some(card));
        }
    }
    Ok(None)
}

/// Replace every `persona` text chunk in a PNG with one uncompressed `iTXt`
/// card placed before `IEND`, preserving every other chunk byte-for-byte.
/// Mirrors TauriTavern's `with_persona` + `replace_text_chunks`.
pub(crate) fn inject_persona(bytes: &[u8], card: &PersonaCard) -> Result<Vec<u8>, String> {
    if !is_png(bytes) {
        return Err("not a PNG image".into());
    }
    let text = serde_json::to_string(card).map_err(|e| e.to_string())?;
    let replacement = build_itxt_chunk(PERSONA_KEYWORD, text.as_bytes());

    let mut out = Vec::with_capacity(bytes.len() + replacement.len() + 16);
    out.extend_from_slice(PNG_SIGNATURE);
    let mut inserted = false;
    for chunk in iter_raw_chunks(bytes)? {
        if &chunk.kind == b"IEND" {
            out.extend_from_slice(&replacement);
            out.extend_from_slice(chunk.raw);
            inserted = true;
            break;
        }
        if matches!(&chunk.kind, b"tEXt" | b"zTXt" | b"iTXt")
            && chunk
                .keyword()
                .is_some_and(|keyword| keyword.eq_ignore_ascii_case(PERSONA_KEYWORD))
        {
            continue;
        }
        out.extend_from_slice(chunk.raw);
    }
    if !inserted {
        return Err("PNG is missing IEND".into());
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// PNG chunk walking
// ---------------------------------------------------------------------------

struct RawChunk<'a> {
    kind: [u8; 4],
    raw: &'a [u8],
    body: &'a [u8],
}

impl<'a> RawChunk<'a> {
    fn keyword(&'a self) -> Option<&'a str> {
        if !matches!(&self.kind, b"tEXt" | b"zTXt" | b"iTXt") {
            return None;
        }
        let end = self.body.iter().position(|byte| *byte == 0)?;
        std::str::from_utf8(&self.body[..end]).ok()
    }
}

fn iter_raw_chunks(data: &[u8]) -> Result<Vec<RawChunk<'_>>, String> {
    let mut chunks = Vec::new();
    let mut offset = PNG_SIGNATURE.len();
    while offset + 8 <= data.len() {
        let length = u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
        let body_end = offset
            .checked_add(8)
            .and_then(|at| at.checked_add(length))
            .ok_or_else(|| "PNG chunk length overflow".to_string())?;
        if body_end + 4 > data.len() {
            return Err("truncated PNG chunk".into());
        }
        let kind = data[offset + 4..offset + 8]
            .try_into()
            .map_err(|_| "bad PNG chunk type".to_string())?;
        chunks.push(RawChunk {
            kind,
            raw: &data[offset..body_end + 4],
            body: &data[offset + 8..body_end],
        });
        offset = body_end + 4;
    }
    // Any trailing bytes mean a truncated final chunk, not a clean PNG end.
    if offset != data.len() {
        return Err("truncated PNG trailer".into());
    }
    Ok(chunks)
}

struct DecodedChunk {
    kind: [u8; 4],
    body: Vec<u8>,
}

fn iter_chunks(data: &[u8]) -> Result<Vec<DecodedChunk>, String> {
    Ok(iter_raw_chunks(data)?
        .into_iter()
        .map(|chunk| DecodedChunk {
            kind: chunk.kind,
            body: chunk.body.to_vec(),
        })
        .collect())
}

/// Decode a text chunk into keyword and text. Compressed `zTXt`/`iTXt`
/// payloads are unsupported (TauriTavern only writes uncompressed `iTXt`) and
/// yield `None`; callers move on to the next chunk.
fn decode_text_chunk(chunk: &DecodedChunk) -> Option<(String, String)> {
    let nul = chunk.body.iter().position(|byte| *byte == 0)?;
    let keyword = String::from_utf8(chunk.body[..nul].to_vec()).ok()?;
    let rest = &chunk.body[nul + 1..];
    match &chunk.kind {
        b"tEXt" => {
            // TauriTavern never writes persona data here; try UTF-8 and skip
            // if the payload is Latin-1 only.
            let text = String::from_utf8(rest.to_vec()).ok()?;
            Some((keyword, text))
        }
        b"iTXt" => {
            if rest.first() != Some(&0) {
                return None; // compression_flag set
            }
            let mut cursor = 2; // compression_flag + compression_method
            cursor += rest[cursor..].iter().position(|byte| *byte == 0)? + 1; // language tag
            cursor += rest[cursor..].iter().position(|byte| *byte == 0)? + 1; // translated keyword
            let text = String::from_utf8(rest[cursor..].to_vec()).ok()?;
            Some((keyword, text))
        }
        _ => None,
    }
}

fn build_itxt_chunk(keyword: &str, text: &[u8]) -> Vec<u8> {
    // keyword \0 compression_flag compression_method language_tag\0
    // translated_keyword\0 text — five zero bytes after the keyword when both
    // tags are empty and compression is disabled.
    let mut body = Vec::with_capacity(keyword.len() + 5 + text.len());
    body.extend_from_slice(keyword.as_bytes());
    body.extend_from_slice(&[0, 0, 0, 0, 0]);
    body.extend_from_slice(text);
    build_chunk(b"iTXt", &body)
}

fn build_chunk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(12 + body.len());
    raw.extend_from_slice(&(body.len() as u32).to_be_bytes());
    raw.extend_from_slice(kind);
    raw.extend_from_slice(body);
    let mut crc_input = Vec::with_capacity(4 + body.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(body);
    raw.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    raw
}

// ---------------------------------------------------------------------------
// CRC-32 (IEEE 802.3, as required by PNG)
// ---------------------------------------------------------------------------

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        let index = ((crc ^ u32::from(*byte)) & 0xff) as usize;
        crc = (crc >> 8) ^ CRC_TABLE[index];
    }
    crc ^ 0xffff_ffff
}

const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut n = 0usize;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn minimal_png() -> Vec<u8> {
        let mut data = PNG_SIGNATURE.to_vec();
        // IHDR (13 zero bytes is fine for chunk-level walking) and IEND.
        data.extend_from_slice(&build_chunk(b"IHDR", &[0u8; 13]));
        data.extend_from_slice(&build_chunk(b"IDAT", b"pixel-bytes"));
        data.extend_from_slice(&build_chunk(b"IEND", &[]));
        data
    }

    #[test]
    fn crc32_matches_ieee_check_vector() {
        // RFC 1952 / PNG required check values; a wrong table would make
        // strict PNG readers reject the synthesized persona chunk.
        assert_eq!(crc32(b""), 0x0000_0000);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn persona_round_trips_through_itxt() {
        let card = PersonaCard::new(
            Some("十一月雨".to_owned()),
            Some(json!({
                "depth": 2,
                "description": "设定正文",
                "lorebook": "",
                "position": 0,
                "role": 0,
                "title": "",
            })),
        );
        let png = inject_persona(&minimal_png(), &card).unwrap();
        let extracted = extract_persona(&png).unwrap().expect("card present");
        assert_eq!(extracted, card);
    }

    #[test]
    fn inject_replaces_existing_persona_chunks() {
        let card_a = PersonaCard::new(Some("旧名".to_owned()), None);
        let card_b = PersonaCard::new(Some("新名".to_owned()), None);
        let once = inject_persona(&minimal_png(), &card_a).unwrap();
        let twice = inject_persona(&once, &card_b).unwrap();

        let mut persona_chunks = 0;
        for chunk in iter_chunks(&twice).unwrap() {
            if let Some((keyword, _)) = decode_text_chunk(&chunk)
                && keyword.eq_ignore_ascii_case(PERSONA_KEYWORD)
            {
                persona_chunks += 1;
            }
        }
        assert_eq!(persona_chunks, 1);
        assert_eq!(
            extract_persona(&twice).unwrap().unwrap().name.as_deref(),
            Some("新名")
        );
    }

    #[test]
    fn non_png_bytes_are_not_cards() {
        assert!(extract_persona(b"not png").unwrap().is_none());
        assert!(inject_persona(b"not png", &PersonaCard::default()).is_err());
    }

    #[test]
    fn truncated_png_is_rejected() {
        let mut data = minimal_png();
        data.truncate(data.len() - 6);
        assert!(extract_persona(&data).is_err());
    }

    #[test]
    fn registry_take_and_insert_round_trips() {
        let mut settings = json!({
            "world_info_settings": { "scan_depth": 4 },
            "power_user": {
                "allow_name1_display": true,
                "personas": { "a.png": "甲", "b.png": "乙" },
                "persona_descriptions": {
                    "a.png": { "description": "甲设定", "position": 0 }
                },
            }
        });

        let registry = take_persona_registry(&mut settings).unwrap();
        assert_eq!(registry.len(), 2);
        assert_eq!(registry["a.png"].name.as_deref(), Some("甲"));
        assert_eq!(
            registry["a.png"]
                .description
                .as_ref()
                .unwrap()
                .get("description"),
            Some(&json!("甲设定"))
        );
        assert!(settings["power_user"].get("personas").is_none());
        assert!(settings["power_user"].get("persona_descriptions").is_none());
        // Other settings survive.
        assert_eq!(settings["world_info_settings"]["scan_depth"], json!(4));

        insert_persona_registry(&mut settings, &registry);
        assert_eq!(settings["power_user"]["personas"]["a.png"], json!("甲"));
        assert_eq!(
            settings["power_user"]["persona_descriptions"]["a.png"]["description"],
            json!("甲设定")
        );
    }

    #[test]
    fn reconcile_follows_deletions_adoptions_and_mtimes() {
        let settings = read_persona_registry(&json!({
            "power_user": {
                "personas": {
                    "gone.png": "已删",
                    "tt-edit.png": "TT旧名",
                    "st-edit.png": "ST新名",
                    "st-only.png": "ST独有",
                }
            }
        }))
        .unwrap();

        let on_disk = vec![
            // TT edited the card at mtime 2000; settings was written at 1000.
            OnDiskPersona {
                id: "tt-edit.png".into(),
                card: Some(PersonaCard::new(Some("TT新名".into()), None)),
                modified_ms: 2000,
            },
            // The card is older than settings: the ST edit wins.
            OnDiskPersona {
                id: "st-edit.png".into(),
                card: Some(PersonaCard::new(Some("卡片旧名".into()), None)),
                modified_ms: 500,
            },
            // Brand-new TT card with no settings entry yet.
            OnDiskPersona {
                id: "new.png".into(),
                card: Some(PersonaCard::new(Some("新角色".into()), None)),
                modified_ms: 2000,
            },
            // ST-created avatar, no card chunk yet: settings stays.
            OnDiskPersona {
                id: "st-only.png".into(),
                card: None,
                modified_ms: 3000,
            },
        ];

        let result = reconcile_persona_registry(&settings, on_disk, 1000);
        assert!(
            !result.contains_key("gone.png"),
            "deleted avatar drops entry"
        );
        assert_eq!(result["tt-edit.png"].name.as_deref(), Some("TT新名"));
        assert_eq!(result["st-edit.png"].name.as_deref(), Some("ST新名"));
        assert_eq!(result["st-only.png"].name.as_deref(), Some("ST独有"));
        assert_eq!(result["new.png"].name.as_deref(), Some("新角色"));
    }

    #[test]
    fn unnamed_shells_count_as_missing_data() {
        let settings = read_persona_registry(&json!({
            "power_user": {
                "personas": {
                    // Shell with a real card that is OLDER than settings.json:
                    // the card must still win (self-heal).
                    "shell-with-card.png": "[Unnamed Persona]",
                    // Shell whose avatar has already been deleted: dropped.
                    "shell-gone.png": "[Unnamed Persona]",
                }
            }
        }))
        .unwrap();

        let on_disk = vec![OnDiskPersona {
            id: "shell-with-card.png".into(),
            card: Some(PersonaCard::new(Some("达达利亚".into()), None)),
            modified_ms: 500,
        }];

        let result = reconcile_persona_registry(&settings, on_disk, 1000);
        assert_eq!(
            result["shell-with-card.png"].name.as_deref(),
            Some("达达利亚"),
            "real card heals the shell regardless of mtime"
        );
        assert!(!result.contains_key("shell-gone.png"));
    }
}
