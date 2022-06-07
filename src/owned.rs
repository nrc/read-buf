use std::cmp;
use std::mem::MaybeUninit;

pub trait OwnedBuf {
    type Cursor<'b>: OwnedCursor<'b>
    where
        Self: 'b;

    /// Returns the total capacity of the buffer.
    fn capacity(&self) -> usize;

    /// Returns the length of the filled part of the buffer.
    fn len(&self) -> usize;

    /// Returns the length of the initialized part of the buffer.
    fn init_len(&self) -> usize;

    /// Returns a shared reference to the filled portion of the buffer.
    fn filled(&self) -> &[u8];

    /// Returns a cursor over the unfilled part of the buffer.
    fn unfilled<'b>(&'b mut self) -> Self::Cursor<'b>;

    /// Clears the buffer, resetting the filled region to empty.
    ///
    /// The number of initialized bytes is not changed, and the contents of the buffer are not modified.
    fn clear(&mut self) -> &mut Self;

    /// Asserts that the first `n` bytes of the buffer are initialized.
    ///
    /// `BorrowBuf` assumes that bytes are never de-initialized, so this method does nothing when called with fewer
    /// bytes than are already known to be initialized.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the first `n` unfilled bytes of the buffer have already been initialized.
    unsafe fn set_init(&mut self, n: usize) -> &mut Self;
}

pub trait OwnedCursor<'a> {
    /// Clone this cursor.
    ///
    /// Since a cursor maintains unique access to its underlying buffer, the cloned cursor is not
    /// accessible while the clone is alive.
    // TODO really don't want a dyn here, but not clear what static type I can use? I want `Self['c/'a]`
    fn clone<'c>(&'c mut self) -> Box<dyn OwnedCursor<'c> + 'c>;

    /// Returns the available space in the cursor.
    fn capacity(&self) -> usize;

    /// Returns the number of bytes written to this cursor since it was created from a `BorrowBuf`.
    ///
    /// Note that if this cursor is a clone of another, then the count returned is the count written
    /// via either cursor, not the count since the cursor was cloned.
    fn written(&self) -> usize;

    /// Returns a shared reference to the initialized portion of the cursor.
    // TODO shouldn't need mut self, but Vec does not have an immutable version of spare_capacity_mut
    fn init_ref(&mut self) -> &[u8];

    /// Returns a mutable reference to the initialized portion of the cursor.
    fn init_mut(&mut self) -> &mut [u8];

    /// Returns a mutable reference to the uninitialized part of the cursor.
    ///
    /// It is safe to uninitialize any of these bytes.
    fn uninit_mut(&mut self) -> &mut [MaybeUninit<u8>];

    /// Returns a mutable reference to the whole cursor.
    ///
    /// # Safety
    ///
    /// The caller must not uninitialize any bytes in the initialized portion of the cursor.
    unsafe fn as_mut(&mut self) -> &mut [MaybeUninit<u8>];

    /// Advance the cursor by asserting that `n` bytes have been filled.
    ///
    /// After advancing, the `n` bytes are no longer accessible via the cursor and can only be
    /// accessed via the underlying buffer. I.e., the buffer's filled portion grows by `n` elements
    /// and its unfilled portion (and the capacity of this cursor) shrinks by `n` elements.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the first `n` bytes of the cursor have been properly
    /// initialised.
    unsafe fn advance(&mut self, n: usize);

    /// Initializes all bytes in the cursor.
    fn ensure_init(&mut self);

    /// Asserts that the first `n` unfilled bytes of the cursor are initialized.
    ///
    /// `BorrowBuf` assumes that bytes are never de-initialized, so this method does nothing when
    /// called with fewer bytes than are already known to be initialized.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the first `n` bytes of the buffer have already been initialized.
    unsafe fn set_init(&mut self, n: usize);

    /// Appends data to the cursor, advancing position within its buffer.
    ///
    /// # Panics
    ///
    /// Panics if `self.capacity()` is less than `buf.len()`.
    fn append(&mut self, buf: &[u8]);
}

// Note that the initialized count is not preserved between cursors.
impl OwnedBuf for Vec<u8> {
    type Cursor<'b> = VecCursor<'b>;

    fn capacity(&self) -> usize {
        self.capacity()
    }

    fn len(&self) -> usize {
        self.len()
    }

    fn init_len(&self) -> usize {
        self.len()
    }

    fn filled(&self) -> &[u8] {
        &**self
    }

    fn unfilled<'b>(&'b mut self) -> Self::Cursor<'b> {
        VecCursor {
            initialized: self.len(),
            start: self.len(),
            buf: self,
        }
    }

    fn clear(&mut self) -> &mut Self {
        self.clear();
        self
    }

    unsafe fn set_init(&mut self, n: usize) -> &mut Self {
        let len = self.len();
        self.set_len(cmp::max(len, n));
        self
    }
}

pub struct VecCursor<'a> {
    buf: &'a mut Vec<u8>,
    // relative to len of buf (not 0)
    initialized: usize,
    start: usize,
}

impl<'a> OwnedCursor<'a> for VecCursor<'a> {
    fn clone<'c>(&'c mut self) -> Box<dyn OwnedCursor<'c> + 'c> {
        Box::new(VecCursor {
            buf: self.buf,
            initialized: self.initialized,
            start: self.start,
        })
    }

    fn capacity(&self) -> usize {
        self.buf.capacity() - self.buf.len()
    }

    fn written(&self) -> usize {
        self.buf.len() - self.start
    }

    fn init_ref(&mut self) -> &[u8] {
        unsafe {
            MaybeUninit::slice_assume_init_ref(&self.buf.spare_capacity_mut()[..self.initialized])
        }
    }

    fn init_mut(&mut self) -> &mut [u8] {
        unsafe {
            MaybeUninit::slice_assume_init_mut(
                &mut self.buf.spare_capacity_mut()[..self.initialized],
            )
        }
    }

    fn uninit_mut(&mut self) -> &mut [MaybeUninit<u8>] {
        &mut self.buf.spare_capacity_mut()[self.initialized..]
    }

    unsafe fn as_mut(&mut self) -> &mut [MaybeUninit<u8>] {
        self.buf.spare_capacity_mut()
    }

    unsafe fn advance(&mut self, n: usize) {
        let len = self.buf.len();
        self.buf.set_len(len + n);
    }

    fn ensure_init(&mut self) {
        for byte in self.uninit_mut() {
            byte.write(0);
        }

        self.initialized = self.buf.capacity();
    }

    unsafe fn set_init(&mut self, n: usize) {
        self.initialized = cmp::max(self.initialized, n);
    }

    fn append(&mut self, buf: &[u8]) {
        let spare = self.buf.spare_capacity_mut();
        assert!(buf.len() <= spare.len());
        MaybeUninit::write_slice(&mut spare[..buf.len()], buf);
        unsafe {
            // SAFETY we just wrote buf.len() bytes
            self.advance(buf.len());
        }
    }
}
