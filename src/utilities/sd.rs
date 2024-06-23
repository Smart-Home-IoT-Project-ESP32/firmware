use chrono::{DateTime, Datelike, Timelike};
use embedded_sdmmc::filesystem::FileError;
use embedded_sdmmc::{
    BlockSpi, Controller, Directory, Error, File, SdMmcError, SdMmcSpi, TimeSource, Timestamp,
    Volume,
};
use esp_idf_hal::sys::{suseconds_t, time_t, timeval};
use log::info;
use messages::Frame;
use std::fmt::Debug;
use std::time::SystemTime;

const FAT_SECTOR_SIZE: usize = 512;
/// File name for the data file
const FILE_NAME: &str = "DATA.txt";

pub struct SD<'a, DR, CS>
where
    CS: embedded_hal_0_2::digital::v2::OutputPin,
    DR: embedded_hal_0_2::blocking::spi::Transfer<u8>,
    DR::Error: Debug,
{
    controller: Controller<BlockSpi<'a, DR, CS>, CurrentTime>,
    volume: Volume,
    directory: Directory,
    file: Option<File>,
    in_buffer: Vec<u8>,
    out_buffer: Vec<u8>,
}

impl<'a, DR, CS> SD<'a, DR, CS>
where
    CS: embedded_hal_0_2::digital::v2::OutputPin,
    DR: embedded_hal_0_2::blocking::spi::Transfer<u8>,
    DR::Error: Debug,
{
    pub fn new(spi_device: &'a mut SdMmcSpi<DR, CS>) -> Result<Self, SDError> {
        // Try and initialise the SDHandle card
        let block_dev = spi_device.acquire()?;
        // Now let's look for volumes (also known as partitions) on our block device.
        let mut controller = Controller::new(block_dev, CurrentTime::new());
        // Try and access Volume 0 (i.e. the first partition)
        let mut volume = controller.get_volume(embedded_sdmmc::VolumeIdx(0))?;
        // Open the root directory
        let directory = controller.open_root_dir(&volume)?;

        // create the file and open it
        let file = controller.open_file_in_dir(
            &mut volume,
            &directory,
            FILE_NAME,
            embedded_sdmmc::Mode::ReadWriteCreateOrAppend,
        )?;

        let sd = Self {
            controller,
            volume,
            directory,
            file: Some(file),
            in_buffer: Vec::new(),
            out_buffer: Vec::new(),
        };

        Ok(sd)
    }

    /// Write a frame to the current file
    pub fn write(&mut self, frame: &Frame) -> Result<(), SDError> {
        let mut frame_vec = frame.serialize();
        let frame_data: heapless::Vec<u8, FAT_SECTOR_SIZE>;

        // write only 512 bytes at a time
        self.in_buffer.append(&mut frame_vec);
        if self.in_buffer.len() >= FAT_SECTOR_SIZE {
            let (buff_first, buff_second) = self.in_buffer.split_at(FAT_SECTOR_SIZE);
            frame_data = buff_first.try_into().unwrap();
            self.in_buffer = buff_second.to_vec();
        } else {
            return Ok(());
        }

        if !self.file.as_ref().unwrap().eof() {
            self.file.as_mut().unwrap().seek_from_end(0)?;
        }

        self.controller.write(
            &mut self.volume,
            self.file.as_mut().unwrap(),
            frame_data.as_slice(),
        )?;

        Ok(())
    }

    /// Read frames from sd card, will read a block of 512 bytes at a time
    pub fn read(&mut self) -> Result<Vec<Frame>, SDError> {
        if self.file.as_ref().unwrap().eof() {
            self.file.as_mut().unwrap().seek_from_start(0)?;
        }

        let mut vec = Vec::new();
        let length = self.file.as_ref().unwrap().length();
        if length != 0 {
            let mut buffer = [0u8; FAT_SECTOR_SIZE];
            // read the first block of the file
            let bytes_read =
                self.controller
                    .read(&self.volume, self.file.as_mut().unwrap(), &mut buffer)?;

            vec.extend_from_slice(&buffer[..bytes_read]);
            vec.append(&mut self.out_buffer);
        }

        // If eof all data has been read, so we can delete the file and create a new one
        if self.file.as_ref().unwrap().eof() && self.file.as_ref().unwrap().length() != 0 {
            info!("All data in the SD card has been read");
            self.controller
                .close_file(&self.volume, self.file.take().unwrap())?;
            self.controller
                .delete_file_in_dir(&self.volume, &self.directory, FILE_NAME)?;
            self.file = Some(self.controller.open_file_in_dir(
                &mut self.volume,
                &self.directory,
                FILE_NAME,
                embedded_sdmmc::Mode::ReadWriteCreateOrAppend,
            )?);
        }

        if !self.in_buffer.is_empty() {
            //println!("Read {} bytes from in_buffer", self.in_buffer.len());
            vec.append(&mut self.in_buffer);
        }

        // Deserialize the frames
        //println!("Deserializing from {:?}", vec);
        let frames = Frame::deserialize_many(&mut vec)?;
        //println!("Remaining buffer is {:?}", vec);
        //println!("Deserialized {:?}", frames);

        // save the remaining bytes
        self.out_buffer = vec;

        Ok(frames)
    }
}

#[derive(Clone, Copy)]
pub struct CurrentTime;

static IS_TIME_SET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl CurrentTime {
    pub fn new() -> Self {
        CurrentTime
    }

    pub fn is_set(&self) -> bool {
        IS_TIME_SET.load(std::sync::atomic::Ordering::Relaxed)
    }

    #[allow(clippy::missing_safety_doc)]
    pub unsafe fn update_time(&self, time: u64) {
        esp_idf_hal::sys::settimeofday(
            &timeval {
                tv_sec: (time / 1000) as time_t,
                tv_usec: ((time % 1000) * 1000) as suseconds_t,
            },
            std::ptr::null(),
        );
        IS_TIME_SET.store(true, std::sync::atomic::Ordering::Relaxed)
    }

    pub fn as_millis_raw(&self) -> u64 {
        // panic if now is before the UNIX epoch, which should never happen
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    pub fn as_millis(&self) -> Option<u64> {
        if self.is_set() {
            Some(self.as_millis_raw())
        } else {
            None
        }
    }
}

impl Default for CurrentTime {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeSource for CurrentTime {
    fn get_timestamp(&self) -> Timestamp {
        let date = DateTime::from_timestamp_millis(self.as_millis_raw() as i64).unwrap();
        Timestamp {
            year_since_1970: (date.year() - 1970) as u8,
            zero_indexed_month: date.month0() as u8,
            zero_indexed_day: date.day0() as u8,
            hours: date.hour() as u8,
            minutes: date.minute() as u8,
            seconds: date.second() as u8,
        }
    }
}

pub enum SDError {
    Error1(Error<SdMmcError>),
    Error2(SdMmcError),
    MessageError(messages::errors::Error),
    FileError(FileError),
    Other(String),
}

impl From<Error<SdMmcError>> for SDError {
    fn from(error: Error<SdMmcError>) -> Self {
        SDError::Error1(error)
    }
}

impl From<SdMmcError> for SDError {
    fn from(error: SdMmcError) -> Self {
        SDError::Error2(error)
    }
}

impl From<messages::errors::Error> for SDError {
    fn from(error: messages::errors::Error) -> Self {
        SDError::MessageError(error)
    }
}

impl From<FileError> for SDError {
    fn from(error: FileError) -> Self {
        SDError::FileError(error)
    }
}

impl Debug for SDError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SDError::Error1(error) => write!(f, "SDError::Error1({:?})", error),
            SDError::Error2(error) => write!(f, "SDError::Error2({:?})", error),
            SDError::MessageError(error) => write!(f, "SDError::MessageError({:?})", error),
            SDError::FileError(error) => write!(f, "SDError::FileError({:?})", error),
            SDError::Other(error) => write!(f, "SDError::Other({:?})", error),
        }
    }
}
