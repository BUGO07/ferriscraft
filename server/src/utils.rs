use serde::{Serialize, de::DeserializeOwned};
use std::{
    collections::VecDeque,
    ops::{Deref, DerefMut},
    path::PathBuf,
};

pub struct Persistent<R: Serialize + DeserializeOwned> {
    pub path: PathBuf,
    pub data: R,
}

impl<R: Serialize + DeserializeOwned> Persistent<R> {
    pub fn new(path: PathBuf, default: R) -> Result<Self, String> {
        if !path.exists() {
            let persistent = Self {
                path,
                data: default,
            };
            persistent.initialize()?;
            persistent.write(&persistent.data)?;
            return Ok(persistent);
        }

        let data = Self::read(&path)?;
        Ok(Self { path, data })
    }

    pub fn persist(&self) -> Result<(), String> {
        self.write(&self.data)?;
        Ok(())
    }

    pub fn update(&mut self, updater: impl FnOnce(&mut R)) -> Result<(), String> {
        updater(&mut self.data);
        self.persist()
    }

    fn initialize(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn read(path: &PathBuf) -> Result<R, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        bincode::deserialize(&bytes).map_err(|e| e.to_string())
    }

    fn write(&self, data: &R) -> Result<(), String> {
        let bytes = bincode::serialize(data).map_err(|e| e.to_string())?;
        std::fs::write(&self.path, bytes).map_err(|e| e.to_string())
    }
}

impl<R: Serialize + DeserializeOwned> Deref for Persistent<R> {
    type Target = R;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<R: Serialize + DeserializeOwned> DerefMut for Persistent<R> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

#[macro_export]
macro_rules! log {
    ($logs:expr, $($arg:tt)*) => ($crate::utils::_log($logs, format_args!($($arg)*)));
}

pub fn _log(logs: &mut VecDeque<String>, args: std::fmt::Arguments) {
    let s = chrono::Local::now().format("[%H:%M:%S] ").to_string() + &args.to_string();
    println!("{}", s);
    if logs.len() == 256 {
        logs.pop_front();
    }
    logs.push_back(s);
}
