pub mod logger {
    use chrono::Local;
    use std::fs::OpenOptions;
    use std::io::prelude::*;

    pub enum LogType {
        Error,
        Warn,
        Info,
        Debug,
    }

    pub struct Logger {
        log_file: String,
    }

    impl Logger {
        pub fn new(log_file: &str) -> Self {
            Logger {
                log_file: log_file.to_string(),
            }
        }

        pub fn log(&self, log_type: LogType, message: &str) {
            let now = Local::now();
            let timestamp = now.format("[%Y-%m-%d %H:%M:%S]").to_string();
            // let timestamp = "dummyDate"; //no me anda chrono
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.log_file)
            {
                let log_message = match log_type {
                    LogType::Error => format!("{} [ERROR] {}\n", timestamp, message),
                    LogType::Warn => format!("{} [WARN] {}\n", timestamp, message),
                    LogType::Info => format!("{} [INFO] {}\n", timestamp, message),
                    LogType::Debug => format!("{} [DEBUG] {}\n", timestamp, message),
                };

                if let Err(e) = file.write_all(log_message.as_bytes()) {
                    println!("ERROR en LOGGER: {}", e);
                }
            } else {
                println!("No se pudo abrir el archivo {}", self.log_file);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::logger::{LogType, Logger};
    use std::fs;

    #[test]
    fn test_logging() {
        let log_file = "test_log.txt";
        let logger = Logger::new(log_file);

        logger.log(LogType::Error, "Error message");
        logger.log(LogType::Warn, "Warning message");
        logger.log(LogType::Info, "Info message");
        logger.log(LogType::Debug, "Debug message");

        let contents = fs::read_to_string(log_file).unwrap();

        assert!(contents.contains("[ERROR] Error message"));
        assert!(contents.contains("[WARN] Warning message"));
        assert!(contents.contains("[INFO] Info message"));
        assert!(contents.contains("[DEBUG] Debug message"));
    }
}
