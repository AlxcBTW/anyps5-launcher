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
    /// Linux: the user cannot open `/dev/udmabuf`.
    DiagnosisNoUdmabuf,
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
    /// About row value and toast: `%@` = the new launcher version.
    LauncherReady,
    /// Toast when a launcher update is downloaded and checked.
    LauncherToast,
    RestartNow,
    /// About row value: `%@` = the new version, but the folder cannot be written to.
    LauncherReadOnly,
    /// Toast when "Restart Now" or the swap on exit fails.
    LauncherUpdateFailed,
    /// Settings row: the launcher answers anyps5:// links and folders.
    Links,
    LinksDetail,
    LinksDetailOther,
    LinksFailed,
    /// Toast: a link or folder the launcher cannot use.
    LinkInvalid,
    /// Toast: `%@` = the title ID.
    LinkNoGame,
    /// Toast: the folder of a link or of the folder menu holds no game.
    LinkNotADump,
    /// Toast: `%@` = the game name.
    LinkOutside,
    /// Toast: `%@` = the game name.
    LinkUnlock,
    /// Toast: `%@` = the game name.
    LinkUnlocked,
    /// Settings row: which toolkit to follow.
    ChannelTitle,
    ChannelStable,
    ChannelPinned,
    ChannelLocal,
    ChannelStableDetail,
    ChannelPinnedDetail,
    ChannelLocalDetail,
    /// Settings row: the release a pinned channel stays on.
    PinnedVersion,
    /// Settings row and panel title.
    WhatsNew,
    /// Detail of Check Now: `%@` = the time.
    LastCheckedOn,
    /// Detail of the toolkit row.
    FitNewer,
    /// Detail of the toolkit row.
    FitOlder,
    /// Toast: `%@` = the release.
    ToolkitAvailable,
    /// Value of a turned-off feature: `%@` = the first release that has it.
    NeedsToolkit,
    /// Value of a turned-off feature: `%@` = the last release without it.
    NeedsNewerThan,
    /// Header of the notes: release, date.
    NotesHeader,
    /// `%@` = how many.
    ChangesCount,
    ChangeOne,
    NotesHighlights,
    NotesOther,
    NotesEverything,
    NotesLoading,
    NotesUnavailable,
    /// Button: the release page on GitHub.
    NotesOpenRelease,
    /// Game sheet: `%@` = the version it was unlocked with, then the installed one.
    UnlockedWith,
    ConversionChanged,
    UnlockOptional,
    UnlockEarlierBuild,
    /// Game sheet row and panel subtitle.
    RecTitle,
    /// Detail of the row: the game does not start without it.
    RecNeeded,
    /// Detail of the row when there is only a warning.
    RecWorthALook,
    /// The one button of the panel.
    RecApply,
    /// Value before a setting is changed.
    RecDefault,
    RecOn,
    RecOff,
    /// Setting name.
    RecPlayGo,
    RecPlayGoInstalled,
    RecPlayGoAbsent,
    /// Setting name.
    RecImageBase,
    /// Setting name.
    RecModuleFolder,
    /// Setting name.
    RecExecutable,
    /// Setting name.
    RecTextureCache,
    /// Setting name.
    RecShaderCache,
    /// Hint under the changes.
    RecUnlockHint,
    /// Toast.
    RecApplied,
    /// Toast.
    RecAppliedUnlock,
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
            Extra::DiagnosisNoUdmabuf => {
                "The game cannot open /dev/udmabuf. Add your user to the kvm group and log in again."
            }
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
                "This folder is not a toolkit. Pick the folder that holds the relinker and libs."
            }
            Extra::Layout => "Layout",
            Extra::LayoutLauncher => "Launcher",
            Extra::LayoutMouse => "Mouse and Keyboard",
            Extra::LayoutGame => "From the Game",
            Extra::LayoutCustom => "Custom",
            Extra::LayoutGameDetail => "Found the game's own PC keys for %@ buttons.",
            Extra::LauncherReady => "%@ ready",
            Extra::LauncherToast => {
                "Launcher %@ is ready. It installs when you close the launcher."
            }
            Extra::RestartNow => "Restart Now",
            Extra::LauncherReadOnly => "%@ available, but this folder is read-only",
            Extra::LauncherUpdateFailed => {
                "Could not install the launcher update. Download it from GitHub instead."
            }
            Extra::Links => "Open AnyPS5 Links with This Launcher",
            Extra::LinksDetail => {
                "Links on websites and in messages open here, and folders get an \"Open with AnyPS5\" entry."
            }
            Extra::LinksDetailOther => {
                "Another copy of the launcher opens them now. Turn this on to use this one."
            }
            Extra::LinksFailed => "Could not change the link settings. Try again.",
            Extra::LinkInvalid => "This is not a valid AnyPS5 link.",
            Extra::LinkNoGame => "%@ is not in your library. Check the games folder in Settings.",
            Extra::LinkNotADump => {
                "This folder is not a game dump. Open the folder that holds eboot.bin."
            }
            Extra::LinkOutside => "%@ is outside your games folder. Move it there to add it.",
            Extra::LinkUnlock => "Select Unlock to prepare %@.",
            Extra::LinkUnlocked => "%@ is already unlocked.",
            Extra::ChannelTitle => "Channel",
            Extra::ChannelStable => "Stable",
            Extra::ChannelPinned => "Pinned",
            Extra::ChannelLocal => "Local Build",
            Extra::ChannelStableDetail => "Follows the newest release.",
            Extra::ChannelPinnedDetail => "Stays on one release. No update is offered.",
            Extra::ChannelLocalDetail => "Uses a folder you built or picked. No update is offered.",
            Extra::PinnedVersion => "Version",
            Extra::WhatsNew => "What's New",
            Extra::LastCheckedOn => "Last checked %@",
            Extra::FitNewer => "Newer than this launcher was tested with. It should still work.",
            Extra::FitOlder => "An older toolkit: some features are turned off.",
            Extra::ToolkitAvailable => "Toolkit %@ is available",
            Extra::NeedsToolkit => "Needs toolkit %@",
            Extra::NeedsNewerThan => "Needs a build newer than %@",
            Extra::NotesHeader => "%@, released %@",
            Extra::ChangesCount => "%@ changes",
            Extra::ChangeOne => "1 change",
            Extra::NotesHighlights => "Highlights",
            Extra::NotesOther => "Other areas",
            Extra::NotesEverything => "Everything",
            Extra::NotesLoading => "Loading the notes…",
            Extra::NotesUnavailable => "The notes could not be loaded.",
            Extra::NotesOpenRelease => "Open on GitHub",
            Extra::UnlockedWith => "Unlocked with %@. %@ is installed.",
            Extra::ConversionChanged => "This update changes how games are converted.",
            Extra::UnlockOptional => "Unlocking again is optional.",
            Extra::UnlockEarlierBuild => "Unlocked with an earlier build of this toolkit.",
            Extra::RecTitle => "Recommended for this game",
            Extra::RecNeeded => "Needed to start",
            Extra::RecWorthALook => "Worth a look",
            Extra::RecApply => "Apply",
            Extra::RecDefault => "Default",
            Extra::RecOn => "On",
            Extra::RecOff => "Off",
            Extra::RecPlayGo => "Missing PlayGo data",
            Extra::RecPlayGoInstalled => "Report as installed",
            Extra::RecPlayGoAbsent => "Report as absent",
            Extra::RecImageBase => "Image base",
            Extra::RecModuleFolder => "Module folder",
            Extra::RecExecutable => "Executable",
            Extra::RecTextureCache => "Texture cache",
            Extra::RecShaderCache => "Shader cache",
            Extra::RecUnlockHint => "Unlock the game again to use this.",
            Extra::RecApplied => "Applied",
            Extra::RecAppliedUnlock => "Saved. Unlock the game again to use it.",
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
            Extra::DiagnosisNoUdmabuf => {
                "Oyun /dev/udmabuf dosyasını açamıyor. Kullanıcını kvm grubuna ekleyip yeniden oturum aç."
            }
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
                "Bu klasör bir araç seti değil. relinker ve libs içeren klasörü seç."
            }
            Extra::Layout => "Düzen",
            Extra::LayoutLauncher => "Başlatıcı",
            Extra::LayoutMouse => "Fare ve klavye",
            Extra::LayoutGame => "Oyundan",
            Extra::LayoutCustom => "Özel",
            Extra::LayoutGameDetail => "Oyunun kendi PC tuşları %@ düğme için bulundu.",
            Extra::LauncherReady => "%@ hazır",
            Extra::LauncherToast => "Başlatıcı %@ hazır. Başlatıcıyı kapattığında kurulur.",
            Extra::RestartNow => "Şimdi yeniden başlat",
            Extra::LauncherReadOnly => "%@ var, ama bu klasör salt okunur",
            Extra::LauncherUpdateFailed => {
                "Başlatıcı güncellemesi kurulamadı. Bunun yerine GitHub'dan indir."
            }
            Extra::Links => "AnyPS5 bağlantılarını bu başlatıcıyla aç",
            Extra::LinksDetail => {
                "Web sitelerindeki ve mesajlardaki bağlantılar burada açılır, klasörlere \"AnyPS5 ile aç\" girişi eklenir."
            }
            Extra::LinksDetailOther => {
                "Şu an bunları başka bir başlatıcı kopyası açıyor. Bunu kullanmak için aç."
            }
            Extra::LinksFailed => "Bağlantı ayarları değiştirilemedi. Yeniden dene.",
            Extra::LinkInvalid => "Bu geçerli bir AnyPS5 bağlantısı değil.",
            Extra::LinkNoGame => "%@ kitaplığında yok. Ayarlar'daki oyun klasörünü denetle.",
            Extra::LinkNotADump => "Bu klasör bir oyun dökümü değil. eboot.bin içeren klasörü aç.",
            Extra::LinkOutside => "%@ oyun klasörünün dışında. Eklemek için oraya taşı.",
            Extra::LinkUnlock => "%@ oyununu hazırlamak için Kilidi Aç'ı seç.",
            Extra::LinkUnlocked => "%@ zaten açık.",
            Extra::ChannelTitle => "Kanal",
            Extra::ChannelStable => "Kararlı",
            Extra::ChannelPinned => "Sabit",
            Extra::ChannelLocal => "Yerel yapı",
            Extra::ChannelStableDetail => "En yeni sürümü izler.",
            Extra::ChannelPinnedDetail => "Tek bir sürümde kalır. Güncelleme önerilmez.",
            Extra::ChannelLocalDetail => {
                "Kendi yaptığın ya da seçtiğin klasörü kullanır. Güncelleme önerilmez."
            }
            Extra::PinnedVersion => "Sürüm",
            Extra::WhatsNew => "Yenilikler",
            Extra::LastCheckedOn => "Son denetleme: %@",
            Extra::FitNewer => {
                "Bu başlatıcının denendiğinden daha yeni. Yine de çalışması beklenir."
            }
            Extra::FitOlder => "Eski bir araç seti: bazı özellikler kapalı.",
            Extra::ToolkitAvailable => "Araç seti %@ indirilebilir",
            Extra::NeedsToolkit => "Araç seti %@ gerekir",
            Extra::NeedsNewerThan => "%@ sürümünden yeni bir yapı gerekir",
            Extra::NotesHeader => "%@, yayın tarihi %@",
            Extra::ChangesCount => "%@ değişiklik",
            Extra::ChangeOne => "1 değişiklik",
            Extra::NotesHighlights => "Öne çıkanlar",
            Extra::NotesOther => "Diğer alanlar",
            Extra::NotesEverything => "Tümü",
            Extra::NotesLoading => "Notlar yükleniyor…",
            Extra::NotesUnavailable => "Notlar yüklenemedi.",
            Extra::NotesOpenRelease => "GitHub'da aç",
            Extra::UnlockedWith => "%@ ile açıldı. %@ kurulu.",
            Extra::ConversionChanged => {
                "Bu güncelleme oyunların dönüştürülme şeklini değiştiriyor."
            }
            Extra::UnlockOptional => "Yeniden açmak isteğe bağlı.",
            Extra::UnlockEarlierBuild => "Bu araç setinin önceki bir yapısıyla açıldı.",
            Extra::RecTitle => "Bu oyun için önerilenler",
            Extra::RecNeeded => "Başlamak için gerekli",
            Extra::RecWorthALook => "Göz atmaya değer",
            Extra::RecApply => "Uygula",
            Extra::RecDefault => "Varsayılan",
            Extra::RecOn => "Açık",
            Extra::RecOff => "Kapalı",
            Extra::RecPlayGo => "Eksik PlayGo verisi",
            Extra::RecPlayGoInstalled => "Kurulu say",
            Extra::RecPlayGoAbsent => "Yok say",
            Extra::RecImageBase => "Yükleme adresi",
            Extra::RecModuleFolder => "Modül klasörü",
            Extra::RecExecutable => "Çalıştırılabilir dosya",
            Extra::RecTextureCache => "Doku önbelleği",
            Extra::RecShaderCache => "Gölgelendirici önbelleği",
            Extra::RecUnlockHint => "Bunun geçerli olması için oyunun kilidini yeniden aç.",
            Extra::RecApplied => "Uygulandı",
            Extra::RecAppliedUnlock => "Kaydedildi. Kullanmak için oyunun kilidini yeniden aç.",
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

    pub const ALL: [Extra; 99] = [
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
        Extra::DiagnosisNoUdmabuf,
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
        Extra::LauncherReady,
        Extra::LauncherToast,
        Extra::RestartNow,
        Extra::LauncherReadOnly,
        Extra::LauncherUpdateFailed,
        Extra::Links,
        Extra::LinksDetail,
        Extra::LinksDetailOther,
        Extra::LinksFailed,
        Extra::LinkInvalid,
        Extra::LinkNoGame,
        Extra::LinkNotADump,
        Extra::LinkOutside,
        Extra::LinkUnlock,
        Extra::LinkUnlocked,
        Extra::ChannelTitle,
        Extra::ChannelStable,
        Extra::ChannelPinned,
        Extra::ChannelLocal,
        Extra::ChannelStableDetail,
        Extra::ChannelPinnedDetail,
        Extra::ChannelLocalDetail,
        Extra::PinnedVersion,
        Extra::WhatsNew,
        Extra::LastCheckedOn,
        Extra::FitNewer,
        Extra::FitOlder,
        Extra::ToolkitAvailable,
        Extra::NeedsToolkit,
        Extra::NeedsNewerThan,
        Extra::NotesHeader,
        Extra::ChangesCount,
        Extra::ChangeOne,
        Extra::NotesHighlights,
        Extra::NotesOther,
        Extra::NotesEverything,
        Extra::NotesLoading,
        Extra::NotesUnavailable,
        Extra::NotesOpenRelease,
        Extra::UnlockedWith,
        Extra::ConversionChanged,
        Extra::UnlockOptional,
        Extra::UnlockEarlierBuild,
        Extra::RecTitle,
        Extra::RecNeeded,
        Extra::RecWorthALook,
        Extra::RecApply,
        Extra::RecDefault,
        Extra::RecOn,
        Extra::RecOff,
        Extra::RecPlayGo,
        Extra::RecPlayGoInstalled,
        Extra::RecPlayGoAbsent,
        Extra::RecImageBase,
        Extra::RecModuleFolder,
        Extra::RecExecutable,
        Extra::RecTextureCache,
        Extra::RecShaderCache,
        Extra::RecUnlockHint,
        Extra::RecApplied,
        Extra::RecAppliedUnlock,
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
