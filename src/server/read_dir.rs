// Copyright 2020 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::io::Result;
use std::os::unix::io::AsRawFd;

use libc::F_DUPFD_CLOEXEC;

use crate::protocol::P9String;

pub struct DirEntry {
    pub ino: libc::ino64_t,
    pub offset: u64,
    pub type_: u8,
    pub name: P9String,
}

pub struct ReadDir {
    dir: *mut libc::DIR,
}

impl Drop for ReadDir {
    fn drop(&mut self) {
        // SAFETY: We know that `self.dir` is a valid pointer allocated by the C library.
        unsafe { libc::closedir(self.dir) };
    }
}

impl ReadDir {
    /// Return the next directory entry. This is implemented as a separate method rather than via
    /// the `Iterator` trait because rust doesn't currently support generic associated types.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<Result<DirEntry>> {
        let dirent64 = unsafe { libc::readdir64(self.dir) };
        if dirent64.is_null() {
            return None;
        }

        // SAFETY: `dirent64` is a non-NULL pointer, as checked above.
        // We trust the C library to return a correctly-aligned, valid pointer.
        let (d_ino, d_off, d_type) =
            unsafe { ((*dirent64).d_ino, (*dirent64).d_off, (*dirent64).d_type) };

        let d_name: &[u8] = unsafe { std::mem::transmute((*dirent64).d_name.as_ref()) };
        let name = match P9String::new(strip_padding(d_name)) {
            Ok(name) => name,
            Err(e) => return Some(Err(e)),
        };

        let entry = DirEntry {
            ino: d_ino,
            offset: d_off as u64,
            type_: d_type,
            name,
        };

        Some(Ok(entry))
    }
}

pub fn read_dir<D: AsRawFd>(dir: &mut D, offset: libc::c_long) -> Result<ReadDir> {
    let dup_fd = unsafe { libc::fcntl(dir.as_raw_fd(), F_DUPFD_CLOEXEC, 0) };
    let dir = unsafe { libc::fdopendir(dup_fd) };
    if dir.is_null() {
        unsafe { libc::close(dup_fd) };
        return Err(std::io::Error::last_os_error());
    }

    let read_dir = ReadDir { dir };

    // Safe because this doesn't modify any memory and we check the return value.
    unsafe { libc::seekdir(read_dir.dir, offset) };

    Ok(read_dir)
}

// Trims any trailing '\0' bytes. Panics if `b` doesn't contain any '\0' bytes.
fn strip_padding(b: &[u8]) -> &[u8] {
    // It would be nice if we could use memchr here but that's locked behind an unstable gate.
    let pos = b
        .iter()
        .position(|&c| c == 0)
        .expect("`b` doesn't contain any nul bytes");

    &b[..pos]
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn padded_cstrings() {
        assert_eq!(strip_padding(b".\0\0\0\0\0\0\0"), b".");
        assert_eq!(strip_padding(b"..\0\0\0\0\0\0"), b"..");
        assert_eq!(strip_padding(b"normal cstring\0"), b"normal cstring");
        assert_eq!(strip_padding(b"\0\0\0\0"), b"");
        assert_eq!(strip_padding(b"interior\0nul bytes\0\0\0"), b"interior");
    }

    #[test]
    #[should_panic(expected = "`b` doesn't contain any nul bytes")]
    fn no_nul_byte() {
        strip_padding(b"no nul bytes in string");
    }
}
