//! Giving memory back to Windows once a window closes: heaps release their
//! free pages (HeapOptimizeResources, then HeapCompact) and the working set
//! is trimmed, so the process returns to its tray baseline.
use windows::Win32::{
    Foundation::HANDLE,
    System::{
        Memory::{
            GetProcessHeaps, HEAP_FLAGS, HeapCompact, HeapOptimizeResources, HeapSetInformation,
            SETPROCESSWORKINGSETSIZEEX_FLAGS, SetProcessWorkingSetSizeEx,
        },
        Threading::GetCurrentProcess,
    },
};

/// HEAP_OPTIMIZE_RESOURCES_INFORMATION (not in the windows crate).
#[repr(C)]
struct OptimizeResources {
    version: u32,
    flags: u32,
}

/// Releases free heap memory and trims the working set. Pages still in use
/// come back on demand (soft faults), so this suits the moment a window
/// has just closed, not a loop.
pub fn trim() {
    let mut heaps = [HANDLE::default(); 64];
    // SAFETY: the buffer holds `heaps.len()` handles; GetProcessHeaps
    // returns how many it needs, which may exceed that.
    let count = unsafe { GetProcessHeaps(&mut heaps) } as usize;
    let options = OptimizeResources {
        version: 1,
        flags: 0,
    };
    for &heap in &heaps[..count.min(heaps.len())] {
        // SAFETY: `heap` is one of this process's heaps; the information
        // block is a valid HEAP_OPTIMIZE_RESOURCES_INFORMATION (version 1) of the size
        // passed.
        unsafe {
            let _ = HeapSetInformation(
                Some(heap),
                HeapOptimizeResources,
                Some(&options as *const _ as *const _),
                size_of::<OptimizeResources>(),
            );
            HeapCompact(heap, HEAP_FLAGS(0));
        }
    }
    // SAFETY: (-1, -1) asks Windows to trim this process's working set.
    unsafe {
        let _ = SetProcessWorkingSetSizeEx(
            GetCurrentProcess(),
            usize::MAX,
            usize::MAX,
            SETPROCESSWORKINGSETSIZEEX_FLAGS(0),
        );
    }
}
