use ttsync_contract::peer::Permissions;

use crate::config::UiLanguage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionPreset {
    ReadOnly,
    ReadWrite,
    ReadWriteMirrorDelete,
}

impl PermissionPreset {
    pub const ALL: [PermissionPreset; 3] = [
        PermissionPreset::ReadOnly,
        PermissionPreset::ReadWrite,
        PermissionPreset::ReadWriteMirrorDelete,
    ];

    pub fn permissions(self) -> Permissions {
        match self {
            PermissionPreset::ReadOnly => Permissions {
                read: true,
                write: false,
                mirror_delete: false,
            },
            PermissionPreset::ReadWrite => Permissions {
                read: true,
                write: true,
                mirror_delete: false,
            },
            PermissionPreset::ReadWriteMirrorDelete => Permissions {
                read: true,
                write: true,
                mirror_delete: true,
            },
        }
    }

    pub fn title(self, lang: UiLanguage) -> &'static str {
        match (lang, self) {
            (UiLanguage::ZhCn, PermissionPreset::ReadOnly) => "只读（Read only）",
            (UiLanguage::ZhCn, PermissionPreset::ReadWrite) => "读写（Read + Write）",
            (UiLanguage::ZhCn, PermissionPreset::ReadWriteMirrorDelete) => {
                "读写 + 允许 Mirror Delete"
            }
            (UiLanguage::En, PermissionPreset::ReadOnly) => "Read only",
            (UiLanguage::En, PermissionPreset::ReadWrite) => "Read + Write",
            (UiLanguage::En, PermissionPreset::ReadWriteMirrorDelete) => {
                "Read + Write + Allow mirror delete"
            }
        }
    }

    pub fn suggest_for(existing: Permissions) -> PermissionPreset {
        PermissionPreset::ALL
            .into_iter()
            .find(|p| p.permissions() == existing)
            .unwrap_or(PermissionPreset::ReadWrite)
    }
}
