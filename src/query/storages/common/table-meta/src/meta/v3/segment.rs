//  Copyright 2023 Datafuse Labs.
//
//  Licensed under the Apache License, Version 2.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
use std::io::Cursor;
use std::sync::Arc;

use common_exception::Result;
use common_io::prelude::BinaryRead;
use serde::Deserialize;
use serde::Serialize;

use crate::meta::format::compress;
use crate::meta::format::encode;
use crate::meta::format::read_and_deserialize;
use crate::meta::format::Compression;
use crate::meta::statistics::FormatVersion;
use crate::meta::v2::BlockMeta;
use crate::meta::Encoding;
use crate::meta::MetaCompression;
use crate::meta::Statistics;
use crate::meta::Versioned;

/// A segment comprises one or more blocks
/// The structure of the segment is the same as that of v2, but the serialization and deserialization methods are different
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct SegmentInfo {
    /// format version of SegmentInfo table meta data
    ///
    /// Note that:
    ///
    /// - A instance of v3::SegmentInfo may have a value of v2/v1::SegmentInfo::VERSION for this field.
    ///
    ///   That indicates this instance is converted from a v2/v1::SegmentInfo.
    ///
    /// - The meta writers are responsible for only writing down the latest version of SegmentInfo, and
    /// the format_version being written is of the latest version.
    ///
    ///   e.g. if the current version of SegmentInfo is v3::SegmentInfo, then the format_version
    ///   that will be written down to object storage as part of SegmentInfo table meta data,
    ///   should always be v3::SegmentInfo::VERSION (which is 3)
    pub format_version: FormatVersion,
    /// blocks belong to this segment
    pub blocks: Vec<Arc<BlockMeta>>,
    /// summary statistics
    pub summary: Statistics,
}

impl SegmentInfo {
    pub fn new(blocks: Vec<Arc<BlockMeta>>, summary: Statistics) -> Self {
        Self {
            format_version: SegmentInfo::VERSION,
            blocks,
            summary,
        }
    }

    // Total block bytes of this segment.
    pub fn total_bytes(&self) -> u64 {
        self.blocks.iter().map(|v| v.block_size).sum()
    }
    #[inline]
    pub fn encoding() -> Encoding {
        Encoding::default()
    }
}

use super::super::v2;

impl SegmentInfo {
    pub fn from_v2(s: v2::SegmentInfo) -> Self {
        // NOTE: it is important to let the format_version return from here
        // carries the format_version of segment info being converted.
        Self {
            format_version: s.format_version,
            blocks: s.blocks,
            summary: s.summary,
        }
    }

    /// Serializes the Segment struct to a byte vector.
    ///
    /// The byte vector contains the format version, encoding, compression, and compressed block data and
    /// summary data. The encoding and compression are set to default values. The block data and summary
    /// data are encoded and compressed, respectively.
    ///
    /// # Returns
    ///
    /// A Result containing the serialized Segment data as a byte vector. If any errors occur during
    /// encoding, compression, or writing to the byte vector, an error will be returned.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let encoding = Encoding::default();
        let compression = Compression::default();

        let blocks = encode(&encoding, &self.blocks)?;
        let blocks_compress = compress(&compression, blocks)?;

        let summary = encode(&encoding, &self.summary)?;
        let summary_compress = compress(&compression, summary)?;

        let data_size = self.format_version.to_le_bytes().len()
            + 2
            + blocks_compress.len().to_le_bytes().len()
            + blocks_compress.len()
            + summary_compress.len().to_le_bytes().len()
            + summary_compress.len();
        let mut buf = Vec::with_capacity(data_size);

        buf.extend_from_slice(&self.format_version.to_le_bytes());
        buf.push(encoding as u8);
        buf.push(compression as u8);
        buf.extend_from_slice(&blocks_compress.len().to_le_bytes());
        buf.extend_from_slice(&summary_compress.len().to_le_bytes());

        buf.extend(blocks_compress);
        buf.extend(summary_compress);

        Ok(buf)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        let mut cursor = Cursor::new(bytes);
        let version = cursor.read_scalar::<u64>()?;
        assert_eq!(version, SegmentInfo::VERSION);
        let encoding = Encoding::try_from(cursor.read_scalar::<u8>()?)?;
        let compression = MetaCompression::try_from(cursor.read_scalar::<u8>()?)?;
        let blocks_size: u64 = cursor.read_scalar::<u64>()?;
        let summary_size: u64 = cursor.read_scalar::<u64>()?;

        let blocks: Vec<Arc<BlockMeta>> =
            read_and_deserialize(&mut cursor, blocks_size, &encoding, &compression)?;
        let summary: Statistics =
            read_and_deserialize(&mut cursor, summary_size, &encoding, &compression)?;

        Ok(Self::new(blocks, summary))
    }
}
