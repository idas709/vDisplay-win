#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Language { #[default] Japanese, English }

impl Language {
    pub fn text<'a>(self, ja: &'a str, en: &'a str) -> &'a str {
        match self { Self::Japanese => ja, Self::English => en }
    }
    pub fn load() -> Self {
        match Self::path().and_then(|p| std::fs::read_to_string(p).ok()).as_deref().map(str::trim) {
            Some("en") => Self::English, _ => Self::Japanese,
        }
    }
    pub fn save(self) -> anyhow::Result<()> {
        let path = Self::path().ok_or_else(|| anyhow::anyhow!(self.text("設定の保存先が見つかりません。", "The settings folder is unavailable.")))?;
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, self.text("ja", "en"))?;
        Ok(())
    }
    fn path() -> Option<std::path::PathBuf> {
        std::env::var_os("LOCALAPPDATA").map(|p| std::path::PathBuf::from(p).join("VirtualDisplayWorkspace").join("language.txt"))
    }
    pub fn error(self, message: &str) -> String {
        if self == Self::Japanese { return message.to_string(); }
        let mut message = message.to_string();
        for (ja,en) in [
            ("仮想ディスプレイが見つかりません。", "The virtual display could not be found."),
            ("仮想ディスプレイが切断されています。Windowsの表示設定で拡張表示にしてください。", "The virtual display is disconnected. Select Extend in Windows display settings."),
            ("仮想ディスプレイが複製表示または未接続のため、解像度を変更できません。Windowsの表示設定で拡張表示にしてください。", "The virtual display is duplicated or disconnected. Select Extend in Windows display settings to change its resolution."),
            ("この解像度は現在の仮想ディスプレイでは使用できません。", "This resolution is not supported by the current virtual display."),
            ("現在の解像度を取得できません。", "Could not read the current resolution."),
            ("解像度を変更できませんでした", "Could not change the resolution"),
            ("Parsec仮想ディスプレイが接続されていません。", "No Parsec virtual display is connected."),
            ("仮想ディスプレイが切断されました。", "The virtual display was disconnected."),
            ("設定ウィンドウを初期化できませんでした。", "Could not initialize the settings window."),
            ("。", "."),
        ] { message = message.replace(ja,en); }
        message
    }
}
