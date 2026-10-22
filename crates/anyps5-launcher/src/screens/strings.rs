//! Texts the Windows port adds to the 212 keys of the original (`ui::i18n`): the toolkit rows of the
//! settings, the CPU conversion choice and the host-CPU diagnosis. English and Turkish like the rest.

use crate::ui::i18n::{Arg, Language, substitute};

/// A Windows-only text. The generated table of the original stays untouched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Extra {
    /// Settings category.
    Toolkit,
    UpdateCheck,
    UpdateCheckDetail,
    CheckNow,
    /// Value while the check runs.
    Checking,
    UpToDate,
    /// `%@` = the new version.
    UpdateAvailable,
    CheckFailed,
    LastChecked,
    NeverChecked,
    ToolkitMissing,
    ToolkitInstall,
    ToolkitInstallUpdate,
    ToolkitChooseFolder,
    CpuConversion,
    CpuConversionDetail,
    CpuAuto,
    CpuNone,
    CpuIntel,
    /// `%@` = the missing CPU features.
    DiagnosisHostCpu,
    SaveReport,
    /// Toast while the toolkit downloads.
    ToolkitDownloading,
    /// Toast: `%@` = the installed version.
    ToolkitReady,
    ToolkitFailed,
    /// Toast: the picked folder has no relinker or libraries.
    ToolkitFolderInvalid,
    /// Settings row: the home screen music toggle.
    HomeMusic,
    HomeMusicDetail,
    /// Settings row: the running game on Discord.
    DiscordPresence,
    DiscordPresenceDetail,
    /// The log box before the game wrote anything.
    EmptyLog,
    /// The button of the removal confirmation (its title already says what is removed).
    Remove,
    /// Controls row: the keyboard layout and its choices.
    Layout,
    LayoutLauncher,
    LayoutMouse,
    /// The PC keys the game's own files name.
    LayoutGame,
    /// The layout after the user moved keys.
    LayoutCustom,
    /// `%@` = how many buttons the game's files give a PC key.
    LayoutGameDetail,
}

impl Extra {
    pub const fn english(self) -> &'static str {
        match self {
            Extra::Toolkit => "Toolkit",
            Extra::UpdateCheck => "Check for Updates",
            Extra::UpdateCheckDetail => {
                "Looks for a newer toolkit when the launcher starts. Only the version number is requested."
            }
            Extra::CheckNow => "Check Now",
            Extra::Checking => "Checking…",
            Extra::UpToDate => "Up to date",
            Extra::UpdateAvailable => "Update available: %@",
            Extra::CheckFailed => "Could not check",
            Extra::LastChecked => "Last Checked",
            Extra::NeverChecked => "Never",
            Extra::ToolkitMissing => "Not installed",
            Extra::ToolkitInstall => "Download the Toolkit",
            Extra::ToolkitInstallUpdate => "Install the Update",
            Extra::ToolkitChooseFolder => "Use a Toolkit Folder",
            Extra::CpuConversion => "CPU Conversion",
            Extra::CpuConversionDetail => {
                "How games are prepared for your processor. Automatic suits most PCs."
            }
            Extra::CpuAuto => "Automatic",
            Extra::CpuNone => "None",
            Extra::CpuIntel => "Intel",
            Extra::DiagnosisHostCpu => "This CPU lacks %@, which the game needs.",
            Extra::SaveReport => "Save Report",
            Extra::ToolkitDownloading => "Downloading the toolkit…",
            Extra::ToolkitReady => "Toolkit %@ is ready",
            Extra::ToolkitFailed => {
                "Could not install the toolkit. Check your connection and try again."
            }
            Extra::HomeMusic => "Home Screen Music",
            Extra::HomeMusicDetail => {
                "Plays quiet ambient music on the home screen. It fades out while a game runs."
            }
            Extra::DiscordPresence => "Show Game on Discord",
            Extra::DiscordPresenceDetail => {
                "With Discord open, your profile shows the game and for how long."
            }
            Extra::EmptyLog => "No log yet. Play the game once and it appears here.",
            Extra::Remove => "Remove",
            Extra::ToolkitFolderInvalid => {
                "This folder is not a toolkit. Pick the folder that holds relinker.exe and libs."
            }
            Extra::Layout => "Layout",
            Extra::LayoutLauncher => "Launcher",
            Extra::LayoutMouse => "Mouse and Keyboard",
            Extra::LayoutGame => "From the Game",
            Extra::LayoutCustom => "Custom",
            Extra::LayoutGameDetail => "Found the game's own PC keys for %@ buttons.",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            Extra::Toolkit => "Araç seti",
            Extra::UpdateCheck => "Güncellemeleri denetle",
            Extra::UpdateCheckDetail => {
                "Başlatıcı açılırken daha yeni bir araç seti arar. Yalnızca sürüm numarası istenir."
            }
            Extra::CheckNow => "Şimdi denetle",
            Extra::Checking => "Denetleniyor…",
            Extra::UpToDate => "Güncel",
            Extra::UpdateAvailable => "Güncelleme var: %@",
            Extra::CheckFailed => "Denetlenemedi",
            Extra::LastChecked => "Son denetleme",
            Extra::NeverChecked => "Hiç",
            Extra::ToolkitMissing => "Kurulu değil",
            Extra::ToolkitInstall => "Araç setini indir",
            Extra::ToolkitInstallUpdate => "Güncellemeyi kur",
            Extra::ToolkitChooseFolder => "Araç seti klasörü kullan",
            Extra::CpuConversion => "CPU dönüştürme",
            Extra::CpuConversionDetail => {
                "Oyunların işlemcine göre nasıl hazırlanacağı. Otomatik çoğu bilgisayara uyar."
            }
            Extra::CpuAuto => "Otomatik",
            Extra::CpuNone => "Yok",
            Extra::CpuIntel => "Intel",
            Extra::DiagnosisHostCpu => "Bu işlemcide oyunun ihtiyaç duyduğu %@ yok.",
            Extra::SaveReport => "Raporu kaydet",
            Extra::ToolkitDownloading => "Araç seti indiriliyor…",
            Extra::ToolkitReady => "Araç seti %@ hazır",
            Extra::ToolkitFailed => "Araç seti kurulamadı. Bağlantını denetleyip yeniden dene.",
            Extra::HomeMusic => "Ana Ekran Müziği",
            Extra::HomeMusicDetail => {
                "Ana ekranda sakin bir ortam müziği çalar. Oyun çalışırken susar."
            }
            Extra::DiscordPresence => "Oyunu Discord'da Göster",
            Extra::DiscordPresenceDetail => {
                "Discord açıksa profilinde oyunun adı ve süresi görünür."
            }
            Extra::EmptyLog => "Henüz günlük yok. Oyunu bir kez oynadığında burada görünür.",
            Extra::Remove => "Kaldır",
            Extra::ToolkitFolderInvalid => {
                "Bu klasör bir araç seti değil. relinker.exe ve libs içeren klasörü seç."
            }
            Extra::Layout => "Düzen",
            Extra::LayoutLauncher => "Başlatıcı",
            Extra::LayoutMouse => "Fare ve klavye",
            Extra::LayoutGame => "Oyundan",
            Extra::LayoutCustom => "Özel",
            Extra::LayoutGameDetail => "Oyunun kendi PC tuşları %@ düğme için bulundu.",
        }
    }

    pub const fn text(self, language: Language) -> &'static str {
        match language {
            Language::Turkish => self.turkish(),
            Language::English => self.english(),
        }
    }

    pub fn format(self, language: Language, args: &[Arg<'_>]) -> String {
        substitute(self.text(language), args)
    }

    pub const ALL: [Extra; 37] = [
        Extra::Toolkit,
        Extra::UpdateCheck,
        Extra::UpdateCheckDetail,
        Extra::CheckNow,
        Extra::Checking,
        Extra::UpToDate,
        Extra::UpdateAvailable,
        Extra::CheckFailed,
        Extra::LastChecked,
        Extra::NeverChecked,
        Extra::ToolkitMissing,
        Extra::ToolkitInstall,
        Extra::ToolkitInstallUpdate,
        Extra::ToolkitChooseFolder,
        Extra::CpuConversion,
        Extra::CpuConversionDetail,
        Extra::CpuAuto,
        Extra::CpuNone,
        Extra::CpuIntel,
        Extra::DiagnosisHostCpu,
        Extra::SaveReport,
        Extra::ToolkitDownloading,
        Extra::ToolkitReady,
        Extra::ToolkitFailed,
        Extra::ToolkitFolderInvalid,
        Extra::HomeMusic,
        Extra::HomeMusicDetail,
        Extra::DiscordPresence,
        Extra::DiscordPresenceDetail,
        Extra::EmptyLog,
        Extra::Remove,
        Extra::Layout,
        Extra::LayoutLauncher,
        Extra::LayoutMouse,
        Extra::LayoutGame,
        Extra::LayoutCustom,
        Extra::LayoutGameDetail,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_text_exists_in_both_languages() {
        for extra in Extra::ALL {
            for language in Language::ALL {
                assert!(!extra.text(language).is_empty(), "{extra:?} {language:?}");
            }
        }
    }

    #[test]
    fn placeholders_match_between_languages() {
        for extra in Extra::ALL {
            assert_eq!(
                extra.english().matches("%@").count(),
                extra.turkish().matches("%@").count(),
                "{extra:?}"
            );
        }
    }
}
