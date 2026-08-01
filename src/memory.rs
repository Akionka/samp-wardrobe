use std::ffi::c_void;
use std::mem::MaybeUninit;
use winapi::um::memoryapi::ReadProcessMemory;
use winapi::um::processthreadsapi::GetCurrentProcess;

/// Types that may be reconstructed from arbitrary bytes read from the game.
///
/// This trait is sealed so callers cannot opt a type with invalid bit patterns
/// (such as `bool`, `char`, or an enum) into `read`. Every implementation is
/// a primitive integer, floating-point value, or raw pointer, for which every
/// bit pattern is valid.
pub trait MemoryValue: private::Sealed + Copy {}

mod private {
    pub trait Sealed {}

    macro_rules! impl_sealed_primitive {
        ($($ty:ty),+ $(,)?) => {
            $(impl Sealed for $ty {})+
        };
    }

    impl_sealed_primitive!(u8, i8, u16, i16, u32, i32, u64, i64, usize, isize, f32, f64,);

    impl<T> Sealed for *const T {}
    impl<T> Sealed for *mut T {}
}

macro_rules! impl_memory_value {
    ($($ty:ty),+ $(,)?) => {
        $(impl MemoryValue for $ty {})+
    };
}

impl_memory_value!(u8, i8, u16, i16, u32, i32, u64, i64, usize, isize, f32, f64,);

impl<T> MemoryValue for *const T {}
impl<T> MemoryValue for *mut T {}

/// Copies memory through Windows instead of dereferencing a game-owned
/// pointer. A stale or unsupported structure then fails the read rather than
/// raising an access violation on GTA's game thread.
/// # Safety
///
/// `output` must point to writable storage for exactly `size` bytes. Callers
/// use owned `MaybeUninit` or `Vec` storage and keep this FFI boundary private.
unsafe fn copy_process_memory(address: usize, output: *mut c_void, size: usize) -> bool {
    if address == 0 || size == 0 || address.checked_add(size).is_none() {
        return false;
    }

    let mut bytes_read = 0_usize;
    let succeeded = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            address as *const winapi::ctypes::c_void,
            output as *mut winapi::ctypes::c_void,
            size,
            &mut bytes_read,
        )
    };
    succeeded != 0 && bytes_read == size
}

/// Reads one plain primitive or raw-pointer value from game-owned memory.
///
/// `ReadProcessMemory` makes an unreadable address a normal `None` result.
/// The sealed `MemoryValue` bound ensures successful bytes always form a
/// valid Rust value; structured game memory must instead be read field by
/// field or with `read_bytes`.
pub fn read<T: MemoryValue>(address: usize) -> Option<T> {
    let mut value = MaybeUninit::<T>::uninit();
    unsafe {
        copy_process_memory(
            address,
            value.as_mut_ptr().cast::<c_void>(),
            std::mem::size_of::<T>(),
        )
    }
    // `MemoryValue` is sealed to types for which every bit pattern is valid.
    .then(|| unsafe { value.assume_init() })
}

pub fn read_bytes(address: usize, size: usize) -> Option<Vec<u8>> {
    let mut bytes = vec![0_u8; size];
    unsafe { copy_process_memory(address, bytes.as_mut_ptr().cast::<c_void>(), size) }
        .then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::read;

    #[test]
    fn reads_a_primitive_from_owned_memory() {
        let value = 0xC0DE_CAFEu32;
        let address = std::ptr::addr_of!(value) as usize;

        assert_eq!(read::<u32>(address), Some(value));
    }
}
