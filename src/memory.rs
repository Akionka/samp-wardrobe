use std::ffi::c_void;
use std::mem::MaybeUninit;
use winapi::um::memoryapi::ReadProcessMemory;
use winapi::um::processthreadsapi::GetCurrentProcess;

/// Copies memory through Windows instead of dereferencing a game-owned
/// pointer. A stale or unsupported structure then fails the read rather than
/// raising an access violation on GTA's game thread.
pub fn copy_process_memory(address: usize, output: *mut c_void, size: usize) -> bool {
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

pub unsafe fn read<T: Copy>(address: usize) -> Option<T> {
    let mut value = MaybeUninit::<T>::uninit();
    copy_process_memory(
        address,
        value.as_mut_ptr().cast::<c_void>(),
        std::mem::size_of::<T>(),
    )
    .then(|| unsafe { value.assume_init() })
}

pub fn read_bytes(address: usize, size: usize) -> Option<Vec<u8>> {
    let mut bytes = vec![0_u8; size];
    copy_process_memory(address, bytes.as_mut_ptr().cast::<c_void>(), size).then_some(bytes)
}
