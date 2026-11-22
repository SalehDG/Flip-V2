use std::path::Path;
use std::process::{Command, Output};

use super::{parse_transfer_note, TransferNoteResult};


#[derive(Debug, Clone)]
pub struct TesseractOptions {
    /// Executable name or full path. Defaults to `tesseract` from PATH.
    pub executable: String,
    /// Language pack code. Flip receipt samples are readable with `eng`.
    pub language: String,
    /// Tesseract page segmentation mode. 6 = uniform text block.
    pub page_segmentation_mode: u8,
}


impl Default for TesseractOptions {
    fn default() -> Self {
        Self {
            executable: "tesseract".into(),
            language: "eng".into(),
            page_segmentation_mode: 6,
        }
    }
}


#[derive(Debug)]
pub enum TesseractError {
    ExecutableNotFound(String),
    ImagePathInvalid(String),
    ProcessFailed { code: Option<i32>, stderr: String },
    EmptyOutput,
}


impl std::fmt::Display for TesseractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExecutableNotFound(name) => write!(f, "Tesseract executable not found: {name}"),
            Self::ImagePathInvalid(path) => write!(f, "Image path is missing or not a file: {path}"),
            Self::ProcessFailed { code, stderr } => {
                write!(f, "Tesseract failed (exit code {code:?}): {stderr}")
            }
            Self::EmptyOutput => write!(f, "Tesseract returned empty OCR text."),
        }
    }
}
impl std::error::Error for TesseractError {}


/// Runs offline Tesseract OCR and parses the transfer-note field.
/// This function does not access Gmail or write to the database.
pub fn run_tesseract(
    image_path: impl AsRef<Path>,
    options: &TesseractOptions,
) -> Result<TransferNoteResult, TesseractError> {
    let image_path = image_path.as_ref();
    if !image_path.is_file() {
        return Err(TesseractError::ImagePathInvalid(image_path.display().to_string()));
    }

    let output = execute_tesseract(image_path, options)?;
    let raw_text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if raw_text.is_empty() {
        return Err(TesseractError::EmptyOutput);
    }

    Ok(parse_transfer_note(raw_text))
}


fn execute_tesseract(image_path: &Path, options: &TesseractOptions) -> Result<Output, TesseractError> {
    let output = Command::new(&options.executable)
        .arg(image_path)
        .arg("stdout")
        .arg("-l")
        .arg(&options.language)
        .arg("--psm")
        .arg(options.page_segmentation_mode.to_string())
        .output()
        .map_err(|_| TesseractError::ExecutableNotFound(options.executable.clone()))?;

    if !output.status.success() {
        return Err(TesseractError::ProcessFailed {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(output)
}
