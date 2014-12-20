#![allow(dead_code)]

use std::io::{IoError, IoErrorKind, Reader};

struct Frame;

// TODO: there should be a way to signal IO error.
// This will be handled by the error reform RFCs, I think.
// For now, use IO error to indicate any failure.
pub type Error = IoError;

fn mk_err() -> Error {
    IoError {
        kind: IoErrorKind::OtherIoError,
        desc: "FLAC decoding error",
        detail: None
    }
}

struct FlacStream {
    streaminfo: StreamInfo,
    metadata_blocks: Vec<MetadataBlock>
}

// TODO: this should be private, but it must be public for the test for now.
pub fn read_stream_header(input: &mut Reader) -> Result<(), Error> {
    // A FLAC stream starts with a 32-bit header 'fLaC' (big endian).
    const HEADER: u32 = 0x66_4c_61_43;
    let header = try!(input.read_be_u32());
    if header != HEADER { return Err(mk_err()); } // TODO: provide more error info.
    Ok(())
}

#[deriving(Copy)]
pub struct StreamInfo {
    pub min_block_size: u16,
    pub max_block_size: u16,
    pub min_frame_size: Option<u32>,
    pub max_frame_size: Option<u32>,
    pub sample_rate: u32,
    pub n_channels: u8,
    pub bits_per_sample: u8,
    pub n_samples: Option<u64>,
    pub md5sum: [u8, ..16]
}

// TODO: should this be private?
#[deriving(Copy)]
pub struct SeekPoint {
    pub sample: u64,
    pub offset: u64,
    pub n_samples: u16
}

// TODO: should this be private?
pub struct SeekTable {
    seekpoints: Vec<SeekPoint>
}

pub enum MetadataBlock {
    StreamInfo(StreamInfo),
    Padding { length: u32 },
    Application { id: u32, data: Vec<u8> },
    SeekTable(SeekTable),
    VorbisComment, // TODO
    CueSheet, // TODO
    Picture // TODO
}

// TODO: this should be private.
#[deriving(Copy)]
pub struct MetadataBlockHeader {
    is_last: bool,
    block_type: u8,
    length: u32
}

// TODO: this should be private, but it must be public for the test for now.
pub fn read_metadata_block_header(input: &mut Reader) -> Result<MetadataBlockHeader, Error> {
    let byte = try!(input.read_u8());

    // The first bit specifies whether this is the last block, the next 7 bits
    // specify the type of the metadata block to follow.
    let is_last = (byte & 1) == 1;
    let block_type = byte >> 1;

    // The length field is 24 bits, or 3 bytes.
    let length = try!(input.read_be_uint_n(3)) as u32;
    
    let header = MetadataBlockHeader {
        is_last: is_last,
        block_type: block_type,
        length: length
    };
    Ok(header)
}

fn read_metadata_block(input: &mut Reader, block_type: u8, length: u32)
                       -> Result<MetadataBlock, Error> {
    match block_type {
        0 => {
            let streaminfo = try!(read_streaminfo_block(input));
            Ok(MetadataBlock::StreamInfo(streaminfo))
        },
        1 => {
            try!(read_padding_block(input, length));
            Ok(MetadataBlock::Padding { length: length })
        },
        2 => {
            let (id, data) = try!(read_application_block(input, length));
            Ok(MetadataBlock::Application { id: id, data: data })
        },
        3 => {
            // TODO: implement seektable reading. For now, pretend it is padding.
            try!(skip_block(input, length));
            Ok(MetadataBlock::Padding { length: length })
        },
        4 => {
            // TODO: implement Vorbis comment reading. For now, pretend it is padding.
            try!(skip_block(input, length));
            Ok(MetadataBlock::Padding { length: length })
        },
        5 => {
            // TODO: implement CUE sheet reading. For now, pretend it is padding.
            try!(skip_block(input, length));
            Ok(MetadataBlock::Padding { length: length })
        },
        6 => {
            // TODO: implement picture reading. For now, pretend it is padding.
            try!(skip_block(input, length));
            Ok(MetadataBlock::Padding { length: length })
        },
        127 => Err(mk_err()), // TODO: "invalid, to avoid confusion with a frame sync code"
        _ => Err(mk_err()) // TODO: "reserved", is that an error or do we ignore it?
    }
}

// TODO: this should be private, but it must be public for the test for now.
pub fn read_streaminfo_block(input: &mut Reader) -> Result<StreamInfo, Error> {
    let min_block_size = try!(input.read_be_u16());
    let max_block_size = try!(input.read_be_u16());

    // The frame size fields are 24 bits, or 3 bytes.
    let min_frame_size = try!(input.read_be_uint_n(3)) as u32;
    let max_frame_size = try!(input.read_be_uint_n(3)) as u32;

    // Next up are 20 bits that determine the sample rate.
    let sample_rate_msb = try!(input.read_be_u16());
    let sample_rate_lsb = try!(input.read_byte());

    // Stitch together the value from the first 16 bits,
    // and then the 4 most significant bits of the next byte.
    let sample_rate = sample_rate_msb as u32 << 4 | sample_rate_lsb as u32 >> 4;

    // Next three bits are the number of channels - 1. Mask them out and add 1.
    let n_channels_bps = sample_rate_lsb;
    let n_channels = ((n_channels_bps >> 1) & 0x7) + 1;

    // The final bit is the most significant of bits per sample - 1. Bits per
    // sample - 1 is 5 bits in total.
    let bps_msb = n_channels_bps & 1;
    let bps_lsb_n_samples = try!(input.read_byte());

    // Stitch together these values, add 1 because # - 1 is stored.
    let bits_per_sample = (bps_msb << 4 | (bps_lsb_n_samples >> 4)) + 1;

    // Number of samples in 36 bits, we have 4 already, 32 to go.
    let n_samples_msb = bps_lsb_n_samples & 0xf;
    let n_samples_lsb = try!(input.read_be_u32());
    let n_samples = n_samples_msb as u64 << 32 | n_samples_lsb as u64;

    let mut md5sum = [0u8, ..16];
    try!(input.read_at_least(16, &mut md5sum));

    let stream_info = StreamInfo {
        min_block_size: min_block_size,
        max_block_size: max_block_size,
        min_frame_size: if min_frame_size == 0 { None } else { Some(min_frame_size) },
        max_frame_size: if max_frame_size == 0 { None } else { Some(max_frame_size) },
        sample_rate: sample_rate,
        n_channels: n_channels,
        bits_per_sample: bits_per_sample,
        n_samples: if n_samples == 0 { None } else { Some(n_samples) },
        md5sum: md5sum
    };

    Ok(stream_info)
}

fn read_padding_block(input: &mut Reader, length: u32) -> Result<(), Error> {
    // The specification dictates that all bits of the padding block must be 0.
    for _ in range(0, length) {
        if try!(input.read_byte()) != 0 {
            return Err(mk_err()); // TODO: error, nonzero bit in padding block.
        }
    }
    Ok(())
}

fn skip_block(input: &mut Reader, length: u32) -> Result<(), Error> {
    // Skip all the padding bytes.
    for _ in range(0, length) { try!(input.read_byte()); }
    Ok(())
}

fn read_application_block(input: &mut Reader, length: u32) -> Result<(u32, Vec<u8>), Error> {
    let id = try!(input.read_be_u32());

    // Four bytes of the block have been used for the ID, the rest is payload.
    let data = try!(input.read_exact((length - 4) as uint));

    Ok((id, data))
}

/// Reads metadata blocks from a stream and exposes them as an iterator.
///
/// It is assumed that the next byte that the reader will read, is the first
/// byte of a metadata block header. This means that the iterator will yield at
/// least a single value. If the iterator ever yields an error, then no more
/// data will be read thereafter, and the next value will be `None`.
struct MetadataBlockReader<'r, R> where R: 'r {
    input: &'r mut R,
    done: bool
}

type MetadataBlockResult = Result<MetadataBlock, Error>;

impl<'r, R> MetadataBlockReader<'r, R> where R: Reader {
    pub fn new(input: &'r mut R) -> MetadataBlockReader<'r, R> {
        MetadataBlockReader { input: input, done: false }
    }

    fn read_next(&mut self) -> MetadataBlockResult {
        let header = try!(read_metadata_block_header(self.input));
        let block = try!(read_metadata_block(self.input, header.block_type,
                                                         header.length));
        self.done = header.is_last;
        Ok(block)
    }
}

impl<'r, R> Iterator<MetadataBlockResult>
    for MetadataBlockReader<'r, R>
    where R: Reader {

    fn next(&mut self) -> Option<MetadataBlockResult> {
        if self.done {
            None
        } else {
            let block = self.read_next();

            // After a failure, no more attempts to read will be made,
            // because we don't know where we are in the stream.
            if !block.is_ok() { self.done = true; }

            Some(block)
        }
    }

    fn size_hint(&self) -> (uint, Option<uint>) {
        if self.done { (0, Some(0)) } else { (1, None) }
    }
}

impl FlacStream {
    pub fn new<R>(input: &mut R) -> Result<FlacStream, Error>
        where R: Reader {
        // A flac stream first of all starts with a stream header.
        try!(read_stream_header(input));

        // Next are one or more metadata blocks. The flac specification
        // dictates that the streaminfo block is the first block. The metadata
        // block reader will yield at least one element, so the unwrap is safe.
        let mut metadata_iter = MetadataBlockReader::new(input);
        let streaminfo_block = try!(metadata_iter.next().unwrap());
        let streaminfo = match streaminfo_block {
            MetadataBlock::StreamInfo(info) => info,
            _ => return Err(mk_err()) // TODO: Error: streaminfo block must be the first block.
        };

        // There might be more metadata blocks, read and store them.
        let mut metadata_blocks = Vec::new();
        for block_result in metadata_iter {
            match block_result {
                Err(error) => return Err(error),
                Ok(block) => metadata_blocks.push(block)
            }
        }

        // Read frames

        let flac_stream = FlacStream {
            streaminfo: streaminfo,
            metadata_blocks: metadata_blocks
        };

        Ok(flac_stream)
    }
}

#[test]
fn test_open_stream() {
    use std::io::File;
    let mut input = File::open(&Path::new("foo.flac")).unwrap();
    let flac_stream = FlacStream::new(&mut input).unwrap();
}
