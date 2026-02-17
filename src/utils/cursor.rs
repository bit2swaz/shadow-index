use eyre::{Result, WrapErr};
use std::fs;
use std::path::{Path, PathBuf};

pub struct CursorManager {
    file_path: PathBuf,
    pub last_processed_block: u64,
}

impl CursorManager {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let file_path = path.as_ref().to_path_buf();

        let last_processed_block = match fs::read_to_string(&file_path) {
            Ok(contents) => {
                let block_num = contents.trim().parse::<u64>().wrap_err_with(|| {
                    format!("failed to parse cursor file as u64: {:?}", file_path)
                })?;

                tracing::info!(
                    "loaded cursor from {:?}: resuming from block {}",
                    file_path,
                    block_num
                );

                block_num
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!(
                    "cursor file {:?} not found, starting from block 0",
                    file_path
                );
                0
            }
            Err(e) => {
                return Err(e)
                    .wrap_err_with(|| format!("failed to read cursor file: {:?}", file_path));
            }
        };

        Ok(Self {
            file_path,
            last_processed_block,
        })
    }

    pub fn update_cursor(&mut self, block_number: u64) -> Result<()> {
        let temp_path = self.file_path.with_extension("tmp");

        fs::write(&temp_path, block_number.to_string())
            .wrap_err_with(|| format!("failed to write temporary cursor file: {:?}", temp_path))?;

        fs::rename(&temp_path, &self.file_path).wrap_err_with(|| {
            format!(
                "failed to rename temp cursor {:?} to {:?}",
                temp_path, self.file_path
            )
        })?;

        self.last_processed_block = block_number;

        tracing::debug!("cursor updated to block {}", block_number);

        Ok(())
    }

    #[cfg(test)]
    pub fn get_last_processed_block(&self) -> u64 {
        self.last_processed_block
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_cursor_manager_new_file() {
        let temp_dir = env::temp_dir();
        let cursor_path = temp_dir.join("shadow-index-test-new.cursor");

        let _ = fs::remove_file(&cursor_path);

        let cursor = CursorManager::new(&cursor_path).expect("failed to create cursor manager");

        assert_eq!(cursor.last_processed_block, 0);
        assert_eq!(cursor.get_last_processed_block(), 0);

        let _ = fs::remove_file(&cursor_path);
    }

    #[test]
    fn test_cursor_manager_update_and_reload() {
        let temp_dir = env::temp_dir();
        let cursor_path = temp_dir.join("shadow-index-test-update.cursor");

        let _ = fs::remove_file(&cursor_path);

        let mut cursor = CursorManager::new(&cursor_path).expect("failed to create cursor manager");
        assert_eq!(cursor.last_processed_block, 0);

        cursor.update_cursor(100).expect("failed to update cursor");
        assert_eq!(cursor.last_processed_block, 100);

        drop(cursor);

        let cursor2 = CursorManager::new(&cursor_path).expect("failed to reload cursor");
        assert_eq!(
            cursor2.last_processed_block, 100,
            "cursor should persist across restarts"
        );

        let _ = fs::remove_file(&cursor_path);
    }

    #[test]
    fn test_cursor_manager_multiple_updates() {
        let temp_dir = env::temp_dir();
        let cursor_path = temp_dir.join("shadow-index-test-multi.cursor");

        let _ = fs::remove_file(&cursor_path);

        let mut cursor = CursorManager::new(&cursor_path).expect("failed to create cursor");

        let blocks = vec![10, 20, 35, 50, 100];
        for block_num in blocks {
            cursor
                .update_cursor(block_num)
                .expect("failed to update cursor");
            assert_eq!(cursor.last_processed_block, block_num);
        }

        drop(cursor);
        let cursor_final = CursorManager::new(&cursor_path).expect("failed to reload");
        assert_eq!(cursor_final.last_processed_block, 100);

        let _ = fs::remove_file(&cursor_path);
    }

    #[test]
    fn test_cursor_atomic_write() {
        let temp_dir = env::temp_dir();
        let cursor_path = temp_dir.join("shadow-index-test-atomic.cursor");

        let _ = fs::remove_file(&cursor_path);

        let mut cursor = CursorManager::new(&cursor_path).expect("failed to create cursor");

        cursor.update_cursor(42).expect("failed to update");

        let temp_path = cursor_path.with_extension("tmp");
        assert!(
            !temp_path.exists(),
            "temporary file should not exist after update"
        );

        assert!(cursor_path.exists(), "cursor file should exist");
        let content = fs::read_to_string(&cursor_path).expect("failed to read cursor");
        assert_eq!(content.trim(), "42");

        let _ = fs::remove_file(&cursor_path);
    }
}
