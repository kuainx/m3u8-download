use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    ITaskbarList3, TaskbarList, TBPF_ERROR, TBPF_INDETERMINATE, TBPF_NOPROGRESS, TBPF_NORMAL,
};

pub(super) struct TaskbarProgress {
    taskbar: Option<ITaskbarList3>,
    hwnd: HWND,
}

impl TaskbarProgress {
    pub(super) fn new(hwnd: HWND) -> Self {
        let taskbar = unsafe {
            windows::Win32::System::Com::CoCreateInstance(
                &TaskbarList,
                None,
                windows::Win32::System::Com::CLSCTX_ALL,
            )
            .ok()
        };
        Self { taskbar, hwnd }
    }

    pub(super) fn set_progress(&self, completed: u64, total: u64) {
        if let Some(ref taskbar) = self.taskbar {
            unsafe {
                let _ = taskbar.SetProgressState(self.hwnd, TBPF_NORMAL);
                let _ = taskbar.SetProgressValue(self.hwnd, completed, total);
            }
        }
    }

    pub(super) fn set_indeterminate(&self) {
        if let Some(ref taskbar) = self.taskbar {
            unsafe {
                let _ = taskbar.SetProgressState(self.hwnd, TBPF_INDETERMINATE);
            }
        }
    }

    pub(super) fn set_error(&self) {
        if let Some(ref taskbar) = self.taskbar {
            unsafe {
                let _ = taskbar.SetProgressState(self.hwnd, TBPF_ERROR);
            }
        }
    }

    pub(super) fn clear(&self) {
        if let Some(ref taskbar) = self.taskbar {
            unsafe {
                let _ = taskbar.SetProgressState(self.hwnd, TBPF_NOPROGRESS);
            }
        }
    }
}
