#![feature(ptr_as_uninit)]
#![feature(maybe_uninit_slice)]
#![feature(maybe_uninit_write_slice)]

use std::mem::MaybeUninit;
use std::cmp;

pub struct ReadBuf<'a> {
    buf: &'a mut [MaybeUninit<u8>],
    filled: usize,
    initialized: usize,
}

impl<'a> ReadBuf<'a> {
    /// Creates a new `ReadBuf` from a fully initialized buffer.
    #[inline]
    pub fn new(buf: &'a mut [u8]) -> ReadBuf<'a> {
        let len = buf.len();

        ReadBuf {
            //SAFETY: initialized data never becoming uninitialized is an invariant of ReadBuf
            buf: unsafe { (buf as *mut [u8]).as_uninit_slice_mut().unwrap() },
            filled: 0,
            initialized: len,
        }
    }

    /// Creates a new `ReadBuf` from a fully uninitialized buffer.
    ///
    /// Use `assume_init` if part of the buffer is known to be already initialized.
    #[inline]
    pub fn uninit(buf: &'a mut [MaybeUninit<u8>]) -> ReadBuf<'a> {
        ReadBuf { buf, filled: 0, initialized: 0 }
    }

    /// Returns the total capacity of the buffer.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    /// Returns a shared reference to the filled portion of the buffer.
    #[inline]
    pub fn filled(&self) -> &[u8] {
        //SAFETY: We only slice the filled part of the buffer, which is always valid
        unsafe { MaybeUninit::slice_assume_init_ref(&self.buf[0..self.filled]) }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.filled
    }

    #[inline]
    pub fn init_len(&self) -> usize {
        self.initialized
    }

    #[inline]
    pub fn unfilled<'b>(&'b mut self) -> ReadCursor<'a, 'b> {
        ReadCursor { buf: self }
    }

    /// Clears the buffer, resetting the filled region to empty.
    ///
    /// The number of initialized bytes is not changed, and the contents of the buffer are not modified.
    #[inline]
    pub fn clear(&mut self) -> &mut Self {
        self.set_filled(0) // The assertion in `set_filled` is optimized out
    }

    /// Sets the size of the filled region of the buffer.
    ///
    /// The number of initialized bytes is not changed.
    ///
    /// Note that this can be used to *shrink* the filled region of the buffer in addition to growing it (for
    /// example, by a `Read` implementation that compresses data in-place).
    ///
    /// # Panics
    ///
    /// Panics if the filled region of the buffer would become larger than the initialized region.
    #[inline]
    pub fn set_filled(&mut self, n: usize) -> &mut Self {
        assert!(n <= self.initialized);

        self.filled = n;
        self
    }

    /// Asserts that the first `n` bytes of the buffer are initialized.
    ///
    /// `ReadBuf` assumes that bytes are never de-initialized, so this method does nothing when called with fewer
    /// bytes than are already known to be initialized.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the first `n` unfilled bytes of the buffer have already been initialized.
    #[inline]
    pub unsafe fn set_init(&mut self, n: usize) -> &mut Self {
        self.initialized = cmp::max(self.initialized, n);
        self
    }
}

pub struct ReadCursor<'a, 'b> {
    buf: &'b mut ReadBuf<'a>,
}

impl<'a, 'b> ReadCursor<'a, 'b> {
    #[inline]
    fn capacity(&self) -> usize {
        self.buf.capacity() - self.buf.filled
    }

    /// Returns a shared reference to the initialized portion of the buffer.
    #[inline]
    pub fn initialized(&self) -> &[u8] {
        //SAFETY: We only slice the initialized part of the buffer, which is always valid
        unsafe { MaybeUninit::slice_assume_init_ref(&self.buf.buf[self.buf.filled..self.buf.initialized]) }
    }

    /// Returns a mutable reference to the initialized portion of the buffer.
    #[inline]
    pub fn initialized_mut(&mut self) -> &mut [u8] {
        //SAFETY: We only slice the initialized part of the buffer, which is always valid
        unsafe { MaybeUninit::slice_assume_init_mut(&mut self.buf.buf[self.buf.filled..self.buf.initialized]) }
    }

    /// Returns a mutable reference to the uninitialized part of the buffer.
    ///
    /// It is safe to uninitialize any of these bytes.
    #[inline]
    pub fn uninitialized_mut(&mut self) -> &mut [MaybeUninit<u8>] {
        &mut self.buf.buf[self.buf.initialized..]
    }

    /// TODO docs
    #[inline]
    pub unsafe fn as_mut(&mut self) -> &mut [MaybeUninit<u8>] {
        &mut self.buf.buf[self.buf.filled..]
    }

    /// Increases the size of the filled region of the buffer.
    #[inline]
    pub fn advance(&mut self, n: usize) -> &mut Self {
        self.buf.filled += n;
        self.buf.initialized = cmp::max(self.buf.initialized, self.buf.filled);
        self
    }

    /// TODO docs
    #[inline]
    pub fn ensure_init(&mut self) -> &mut Self {
        for byte in self.uninitialized_mut() {
            byte.write(0);
        }
        self.buf.initialized = self.buf.capacity();

        self
    }

    /// Asserts that the first `n` unfilled bytes of the buffer are initialized.
    ///
    /// `ReadBuf` assumes that bytes are never de-initialized, so this method does nothing when called with fewer
    /// bytes than are already known to be initialized.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the first `n` unfilled bytes of the buffer have already been initialized.
    #[inline]
    pub unsafe fn assume_init(&mut self, n: usize) -> &mut Self {
        self.buf.initialized = cmp::max(self.buf.initialized, self.buf.filled + n);
        self
    }

    /// Appends data to the buffer, advancing the written position and possibly also the initialized position.
    ///
    /// # Panics
    ///
    /// Panics if `self.unfilled().len()` is less than `buf.len()`.
    #[inline]
    pub fn append(&mut self, buf: &[u8]) {
        assert!(self.capacity() >= buf.len());

        // SAFETY: we do not de-initialize any of the elements of the slice
        unsafe {
            MaybeUninit::write_slice(&mut self.as_mut()[..buf.len()], buf);
        }

        // SAFETY: We just added the entire contents of buf to the filled section.
        unsafe {
            self.assume_init(buf.len());
        }
        self.buf.filled += buf.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read<'a, 'b>(mut buf: ReadCursor<'a, 'b>) -> Result<(), ()> {
        let init = buf.initialize_to(4);
        init[1] = 1;
        init[2] = 2;
        init[3] = 3;
        buf.advance(4);
        Ok(())
    }

    #[test]
    fn it_works() {
        let mut backing = Vec::with_capacity(32);
        let mut buf = ReadBuf::uninit(backing.spare_capacity_mut());
        {
            let cursor = buf.unfilled();
            read(cursor).unwrap();
        }

        let len = buf.len();
        unsafe {
            backing.set_len(len);
        }

        assert_eq!(backing.len(), 4);
        assert_eq!(backing[0], 0);
        assert_eq!(backing[1], 1);
        assert_eq!(backing[2], 2);
        assert_eq!(backing[3], 3);
    }
}
