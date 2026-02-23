#[derive(Debug)]
pub struct MmapFile {
    _file: std::fs::File,
    mmap: memmap2::Mmap,
    path: std::path::PathBuf,
}

impl MmapFile {
    /// # Errors
    ///
    /// - `std::io::Error` if the file cannot be opened or mapped.
    pub fn open(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        let path_buf = path.as_ref().to_path_buf();
        let file = std::fs::File::open(&path_buf)?;

        // SAFETY:
        // - File is opened read-only
        // - We keep the file handle alive in struct
        // - Caller only gets immutable &[u8]
        let mmap = unsafe { memmap2::Mmap::map(&file)? };

        Ok(Self {
            _file: file,
            mmap,
            path: path_buf,
        })
    }

    /// STRICT: Gets an exact slice of bytes.
    /// Returns `None` if the requested range goes out of bounds or overflows.
    /// Use this when your piece table logic *guarantees* the bounds are correct.
    #[inline]
    #[must_use]
    pub fn get_bytes_exact(&self, start: usize, length: usize) -> Option<&[u8]> {
        // checked_add prevents integer overflow panics if `length` is huge
        let end = start.checked_add(length)?;

        // .get() does safe bounds checking and returns Option<&[u8]>
        self.mmap.get(start..end)
    }

    /// FORGIVING: Gets bytes starting at `start`, up to `length`.
    /// If `length` goes past the end of the file, it just returns the rest of the file.
    /// If `start` is past the end of the file, it returns an empty slice.
    #[inline]
    #[must_use]
    pub fn get_bytes_clamped(&self, start: usize, length: usize) -> &[u8] {
        if start >= self.len() {
            return &[];
        }

        // saturating_add prevents overflow, min caps it at file length
        let end = std::cmp::min(start.saturating_add(length), self.len());

        // Safe to index directly here because we manually clamped `end`
        &self.mmap[start..end]
    }

    #[inline]
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.mmap
    }

    /// File length in bytes.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.mmap.len()
    }

    /// Whether file is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Path of mapped file.
    #[inline]
    #[must_use]
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}
