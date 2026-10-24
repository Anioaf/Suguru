use std::{fs, path::PathBuf, time::SystemTime};

pub struct ShaderFile {
    main: PathBuf,
    common: PathBuf,
    fallback_main: &'static str,
    fallback_common: &'static str,
    stamp: Option<SystemTime>,
}

impl ShaderFile {
    pub fn new(main: PathBuf, common: PathBuf, fallback_main: &'static str, fallback_common: &'static str) -> Self {
        Self { main, common, fallback_main, fallback_common, stamp: None }
    }

    fn newest(&self) -> Option<SystemTime> {
        [&self.main, &self.common]
            .into_iter()
            .filter_map(|p| fs::metadata(p).and_then(|m| m.modified()).ok())
            .max()
    }

    pub fn changed(&self) -> bool {
        self.newest() != self.stamp
    }

    pub fn read(&mut self) -> String {
        self.stamp = self.newest();
        let common = fs::read_to_string(&self.common).unwrap_or_else(|_| self.fallback_common.to_owned());
        let main = fs::read_to_string(&self.main).unwrap_or_else(|_| self.fallback_main.to_owned());
        format!("{common}\n{main}")
    }
}
