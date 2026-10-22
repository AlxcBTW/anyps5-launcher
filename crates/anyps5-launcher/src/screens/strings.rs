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
    /// Toast: `%@` = the reason.
    ToolkitFailed,
}

impl Extra {
    pub const fn english(self) -> &'static str {
        match self {
            Extra::Toolkit => "Toolkit",
            Extra::UpdateCheck => "Check for updates",
            Extra::UpdateCheckDetail => {
                "Looks for a newer toolkit when the launcher starts. Only the version number is requested."
            }
            Extra::CheckNow => "Check now",
            Extra::Checking => "Checking…",
            Extra::UpToDate => "Up to date",
            Extra::UpdateAvailable => "Update available: %@",
            Extra::CheckFailed => "Could not check",
            Extra::LastChecked => "Last checked",
            Extra::NeverChecked => "Never",
            Extra::ToolkitMissing => "Not installed",
            Extra::ToolkitInstall => "Download the toolkit",
            Extra::ToolkitInstallUpdate => "Install the update",
            Extra::ToolkitChooseFolder => "Use a toolkit folder",
            Extra::CpuConversion => "CPU conversion",
            Extra::CpuConversionDetail => {
                "How games are prepared for your processor. Automatic suits most PCs."
            }
            Extra::CpuAuto => "Automatic",
            Extra::CpuNone => "None",
            Extra::CpuIntel => "Intel",
            Extra::DiagnosisHostCpu => "This CPU lacks %@, which the game needs.",
            Extra::SaveReport => "Save report",
            Extra::ToolkitDownloading => "Downloading the toolkit…",
            Extra::ToolkitReady => "Toolkit %@ is ready",
            Extra::ToolkitFailed => "Toolkit not installed: %@",
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
            Extra::ToolkitFailed => "Araç seti kurulamadı: %@",
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

    pub const ALL: [Extra; 24] = [
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
