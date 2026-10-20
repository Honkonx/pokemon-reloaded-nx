//! Horizon runtime glue: global allocator over newlib's `malloc` and a panic
//! handler. Only compiled with the `switch` feature; the C++ entrypoint links
//! newlib/libnx, which provide these symbols.

use core::alloc::{GlobalAlloc, Layout};
use core::ffi::c_void;

extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
    fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void;
    fn memalign(align: usize, size: usize) -> *mut c_void;
}

struct NewlibAllocator;

// newlib's malloc is aligned to 8 bytes on AArch64 newlib builds; larger
// alignments go through memalign (devkitA64's newlib has no posix_memalign).
const MALLOC_ALIGN: usize = 8;

unsafe impl GlobalAlloc for NewlibAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.align() <= MALLOC_ALIGN {
            malloc(layout.size()).cast()
        } else {
            memalign(layout.align(), layout.size()).cast()
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        free(ptr.cast());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if layout.align() <= MALLOC_ALIGN {
            realloc(ptr.cast(), new_size).cast()
        } else {
            let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
            let new_ptr = self.alloc(new_layout);
            if !new_ptr.is_null() {
                core::ptr::copy_nonoverlapping(ptr, new_ptr, layout.size().min(new_size));
                self.dealloc(ptr, layout);
            }
            new_ptr
        }
    }
}

#[global_allocator]
static ALLOCATOR: NewlibAllocator = NewlibAllocator;

extern "C" {
    fn pokenx_host_log(line: *const u8);
    fn pokenx_host_random(buffer: *mut u8, length: usize);
    fn exit(code: i32) -> !;
}

fn fill_random(buffer: &mut [u8]) -> Result<(), getrandom::Error> {
    // SAFETY: `buffer` is valid for `buffer.len()` bytes.
    unsafe { pokenx_host_random(buffer.as_mut_ptr(), buffer.len()) };
    Ok(())
}

getrandom::register_custom_getrandom!(fill_random);

/// Logs the panic message to the SD log and leaves the process (returns to the launcher),
/// instead of spinning forever.
#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    use core::fmt::Write;
    let mut line = crate::logging::LineBuf::new();
    let _ = write!(line, "PANIC: {info}");
    // SAFETY: the line is NUL-terminated; `exit` runs newlib's atexit handlers.
    unsafe {
        pokenx_host_log(line.as_c_ptr().cast());
        exit(70)
    }
}
