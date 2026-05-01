//! FLV file writer for recording streams to disk

use super::{FlvEncoder, FlvError, FlvResult};
use super::encoder::ScriptData;
use crate::media::MediaFrame;
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info};

/// FLV file writer for recording streams
///
/// Supports:
/// - Recording live streams to FLV files
/// - Automatic file rotation based on size or duration
/// - Async I/O for non-blocking recording
pub struct FlvWriter {
    encoder: FlvEncoder,
    file: Option<File>,
    path: std::path::PathBuf,
    bytes_written: u64,
    frames_written: u64,
    started_at: Option<std::time::Instant>,
    config: WriterConfig,
}

/// Configuration for FLV writer
#[derive(Debug, Clone)]
pub struct WriterConfig {
    /// Maximum file size before rotation (0 = no limit)
    pub max_size: u64,
    /// Maximum duration before rotation (0 = no limit)
    pub max_duration: std::time::Duration,
    /// Write buffer size
    pub buffer_size: usize,
    /// Whether to write sequence headers at start
    pub write_sequence_headers: bool,
    /// Metadata to include
    pub metadata: Option<ScriptData>,
}

impl Default for WriterConfig {
    fn default() -> Self {
        Self {
            max_size: 0,
            max_duration: std::time::Duration::ZERO,
            buffer_size: 64 * 1024, // 64KB
            write_sequence_headers: true,
            metadata: None,
        }
    }
}

impl WriterConfig {
    pub fn with_rotation_size(mut self, size_mb: u64) -> Self {
        self.max_size = size_mb * 1024 * 1024;
        self
    }

    pub fn with_rotation_duration(mut self, duration: std::time::Duration) -> Self {
        self.max_duration = duration;
        self
    }

    pub fn with_metadata(mut self, metadata: ScriptData) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

impl FlvWriter {
    /// Create a new FLV writer
    pub async fn new<P: AsRef<Path>>(
        path: P,
        has_video: bool,
        has_audio: bool,
    ) -> FlvResult<Self> {
        Self::with_config(path, has_video, has_audio, WriterConfig::default()).await
    }

    /// Create a new FLV writer with configuration
    pub async fn with_config<P: AsRef<Path>>(
        path: P,
        has_video: bool,
        has_audio: bool,
        config: WriterConfig,
    ) -> FlvResult<Self> {
        let path = path.as_ref().to_path_buf();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                FlvError::Io(e)
            })?;
        }

        let file = File::create(&path).await.map_err(|e| FlvError::Io(e))?;

        info!(path = %path.display(), "FLV writer created");

        Ok(Self {
            encoder: FlvEncoder::new(has_video, has_audio),
            file: Some(file),
            path,
            bytes_written: 0,
            frames_written: 0,
            started_at: None,
            config,
        })
    }

    /// Start recording
    ///
    /// Writes header and metadata
    pub async fn start(&mut self) -> FlvResult<()> {
        if let Some(ref mut file) = self.file {
            // Write header
            if let Some(header) = self.encoder.header() {
                file.write_all(&header).await.map_err(|e| FlvError::Io(e))?;
                self.bytes_written += header.len() as u64;
            }

            // Write metadata if provided
            if let Some(ref metadata) = self.config.metadata {
                let data = self.encoder.encode_metadata(metadata)?;
                file.write_all(&data).await.map_err(|e| FlvError::Io(e))?;
                self.bytes_written += data.len() as u64;
            }

            self.started_at = Some(std::time::Instant::now());

            info!(path = %self.path.display(), "FLV recording started");
        }

        Ok(())
    }

    /// Write a media frame
    pub async fn write_frame(&mut self, frame: &MediaFrame) -> FlvResult<()> {
        // Check if rotation is needed before getting file reference
        if self.should_rotate() {
            self.rotate().await?;
        }

        let file = self.file.as_mut().ok_or_else(|| {
            FlvError::InvalidData("Writer not started".into())
        })?;

        // Encode frame
        let data = self.encoder.encode_frame(frame)?.ok_or_else(|| {
            FlvError::InvalidData("Failed to encode frame".into())
        })?;

        // Write to file
        file.write_all(&data).await.map_err(|e| FlvError::Io(e))?;

        self.bytes_written += data.len() as u64;
        self.frames_written += 1;

        debug!(
            path = %self.path.display(),
            frames = self.frames_written,
            bytes = self.bytes_written,
            "Frame written"
        );

        Ok(())
    }

    /// Write multiple frames (batch write for efficiency)
    pub async fn write_frames(&mut self, frames: &[MediaFrame]) -> FlvResult<()> {
        for frame in frames {
            self.write_frame(frame).await?;
        }
        Ok(())
    }

    /// Check if file rotation is needed
    pub fn should_rotate(&self) -> bool {
        // Check size limit
        if self.config.max_size > 0 && self.bytes_written >= self.config.max_size {
            return true;
        }

        // Check duration limit
        if self.config.max_duration > std::time::Duration::ZERO {
            if let Some(started) = self.started_at {
                if started.elapsed() >= self.config.max_duration {
                    return true;
                }
            }
        }

        false
    }

    /// Rotate to a new file
    async fn rotate(&mut self) -> FlvResult<()> {
        // Generate new filename with timestamp
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let stem = self.path.file_stem().unwrap_or_default();
        let ext = self.path.extension().unwrap_or_default();
        let new_name = format!("{}_{}.{}", stem.to_string_lossy(), timestamp, ext.to_string_lossy());

        let new_path = self.path.with_file_name(new_name);

        info!(
            old_path = %self.path.display(),
            new_path = %new_path.display(),
            "Rotating FLV file"
        );

        // Close current file
        if let Some(file) = self.file.take() {
            file.sync_all().await.map_err(|e| FlvError::Io(e))?;
        }

        // Create new file
        let new_file = File::create(&new_path).await.map_err(|e| FlvError::Io(e))?;

        // Update self
        self.file = Some(new_file);
        self.path = new_path;
        self.bytes_written = 0;
        self.frames_written = 0;
        self.started_at = Some(std::time::Instant::now());

        // Write header to new file
        if let Some(ref mut file) = self.file {
            if let Some(header) = self.encoder.header() {
                file.write_all(&header).await.map_err(|e| FlvError::Io(e))?;
                self.bytes_written += header.len() as u64;
            }
        }

        Ok(())
    }

    /// Flush data to disk
    pub async fn flush(&mut self) -> FlvResult<()> {
        if let Some(ref mut file) = self.file {
            file.flush().await.map_err(|e| FlvError::Io(e))?;
        }
        Ok(())
    }

    /// Stop recording and close file
    pub async fn stop(&mut self) -> FlvResult<()> {
        if let Some(file) = self.file.take() {
            file.sync_all().await.map_err(|e| FlvError::Io(e))?;

            info!(
                path = %self.path.display(),
                frames = self.frames_written,
                bytes = self.bytes_written,
                "FLV recording stopped"
            );
        }
        Ok(())
    }

    /// Get current file path
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Get bytes written
    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    /// Get frames written
    pub fn frames_written(&self) -> u64 {
        self.frames_written
    }

    /// Get recording duration
    pub fn duration(&self) -> Option<std::time::Duration> {
        self.started_at.map(|t| t.elapsed())
    }
}

/// Rotating FLV writer that manages multiple files
pub struct RotatingFlvWriter {
    base_path: std::path::PathBuf,
    has_video: bool,
    has_audio: bool,
    config: WriterConfig,
    current_writer: Option<FlvWriter>,
    files_created: u32,
}

impl RotatingFlvWriter {
    pub async fn new<P: AsRef<Path>>(
        base_path: P,
        has_video: bool,
        has_audio: bool,
        config: WriterConfig,
    ) -> FlvResult<Self> {
        Ok(Self {
            base_path: base_path.as_ref().to_path_buf(),
            has_video,
            has_audio,
            config,
            current_writer: None,
            files_created: 0,
        })
    }

    /// Start recording
    pub async fn start(&mut self) -> FlvResult<()> {
        self.create_new_file().await
    }

    /// Write a frame
    pub async fn write_frame(&mut self, frame: &MediaFrame) -> FlvResult<()> {
        // Check if current writer needs rotation
        if let Some(ref writer) = self.current_writer {
            if writer.should_rotate() {
                self.create_new_file().await?;
            }
        }

        if let Some(ref mut writer) = self.current_writer {
            writer.write_frame(frame).await
        } else {
            Err(FlvError::InvalidData("Writer not started".into()))
        }
    }

    /// Create a new file
    async fn create_new_file(&mut self) -> FlvResult<()> {
        // Close current writer
        if let Some(mut writer) = self.current_writer.take() {
            writer.stop().await?;
        }

        // Generate new filename
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let stem = self.base_path.file_stem().unwrap_or_default();
        let ext = self.base_path.extension().unwrap_or_default();

        let new_name = if self.files_created == 0 {
            format!("{}.{}",
                stem.to_string_lossy(),
                ext.to_string_lossy()
            )
        } else {
            format!("{}_{}_{}.{}",
                stem.to_string_lossy(),
                timestamp,
                self.files_created,
                ext.to_string_lossy()
            )
        };

        let new_path = self.base_path.with_file_name(new_name);

        // Create new writer
        let mut writer = FlvWriter::with_config(
            &new_path,
            self.has_video,
            self.has_audio,
            self.config.clone(),
        ).await?;

        writer.start().await?;

        self.current_writer = Some(writer);
        self.files_created += 1;

        Ok(())
    }

    /// Stop recording
    pub async fn stop(&mut self) -> FlvResult<()> {
        if let Some(mut writer) = self.current_writer.take() {
            writer.stop().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{CodecType, Timestamp, VideoFrameType};

    #[tokio::test]
    async fn test_flv_writer() {
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test.flv");

        {
            let mut writer = FlvWriter::new(&test_file, true, false).await.unwrap();
            writer.start().await.unwrap();

            let frame = MediaFrame::video(
                1,
                Timestamp::from_millis(1000),
                VideoFrameType::Keyframe,
                CodecType::H264,
                bytes::Bytes::from(vec![0x65, 0x88]),
            );

            writer.write_frame(&frame).await.unwrap();
            writer.stop().await.unwrap();
        }

        // Verify file was created
        assert!(test_file.exists());

        // Cleanup
        tokio::fs::remove_file(&test_file).await.ok();
    }
}
