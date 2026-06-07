use x11_dl::xlib::{self, Xlib};

/// Manages an X11 child window for embedding mpv into the egui viewport.
pub struct EmbeddedWindow {
    xlib: Xlib,
    display: *mut xlib::Display,
    parent: u64,
    child: u64,
    active: bool,
}

impl EmbeddedWindow {
    /// Create a new embedded window manager. No X11 operations yet.
    pub fn new() -> Self {
        let xlib = Xlib::open().expect("Failed to load Xlib");
        Self {
            xlib,
            display: std::ptr::null_mut(),
            parent: 0,
            child: 0,
            active: false,
        }
    }

    /// Initialize the X11 connection and parent window reference.
    /// Call once when the eframe native window is known.
    pub fn init(&mut self, parent_xid: u64) {
        if self.active {
            return;
        }
        self.parent = parent_xid;
        // Open our own display connection (safer than sharing winit's)
        self.display = unsafe { (self.xlib.XOpenDisplay)(std::ptr::null()) };
        if self.display.is_null() {
            log::error!("Failed to open X11 display for mpv embedding");
        }
    }

    /// Create the child window at the given position and size (physical pixels).
    /// Returns the child window XID to pass to mpv --wid.
    pub fn create(&mut self, x: i32, y: i32, w: u32, h: u32) -> Option<u64> {
        if self.display.is_null() || self.parent == 0 {
            return None;
        }

        // Destroy old child if exists
        self.destroy_inner();

        let w = if w < 1 { 1 } else { w };
        let h = if h < 1 { 1 } else { h };

        let child = unsafe {
            (self.xlib.XCreateSimpleWindow)(
                self.display,
                self.parent,
                x,
                y,
                w,
                h,
                0, // border width
                0, // border (black)
                0, // background (black — mpv will paint over it)
            )
        };

        if child == 0 {
            log::error!("XCreateSimpleWindow failed");
            return None;
        }

        // Select input so we can receive events
        unsafe {
            (self.xlib.XSelectInput)(
                self.display,
                child,
                xlib::ExposureMask | xlib::StructureNotifyMask,
            );
            (self.xlib.XMapWindow)(self.display, child);
            (self.xlib.XSync)(self.display, 0);
        }

        self.child = child;
        self.active = true;
        log::info!("Created X11 child window 0x{:x} at ({},{}) {}x{}", child, x, y, w, h);
        Some(child)
    }

    /// Reposition and resize the child window (physical pixel coordinates).
    pub fn reposition(&self, x: i32, y: i32, w: u32, h: u32) {
        if !self.active || self.child == 0 || self.display.is_null() {
            return;
        }
        let w = if w < 1 { 1 } else { w };
        let h = if h < 1 { 1 } else { h };
        unsafe {
            (self.xlib.XMoveResizeWindow)(self.display, self.child, x, y, w, h);
            (self.xlib.XSync)(self.display, 0);
        }
    }

    /// Hide the child window (without destroying it).
    #[allow(dead_code)]
    pub fn hide(&self) {
        if !self.active || self.child == 0 || self.display.is_null() {
            return;
        }
        unsafe {
            (self.xlib.XUnmapWindow)(self.display, self.child);
            (self.xlib.XSync)(self.display, 0);
        }
    }

    /// Show the child window.
    #[allow(dead_code)]
    pub fn show(&self) {
        if !self.active || self.child == 0 || self.display.is_null() {
            return;
        }
        unsafe {
            (self.xlib.XMapWindow)(self.display, self.child);
            (self.xlib.XSync)(self.display, 0);
        }
    }

    /// Check if the child window is active.
    pub fn is_active(&self) -> bool {
        self.active && self.child != 0
    }

    fn destroy_inner(&mut self) {
        if self.child != 0 && !self.display.is_null() {
            unsafe {
                (self.xlib.XDestroyWindow)(self.display, self.child);
                (self.xlib.XSync)(self.display, 0);
            }
        }
        self.child = 0;
        self.active = false;
    }

    /// Destroy the child window and clean up.
    pub fn destroy(&mut self) {
        self.destroy_inner();
    }
}

impl Drop for EmbeddedWindow {
    fn drop(&mut self) {
        self.destroy_inner();
        if !self.display.is_null() {
            unsafe {
                (self.xlib.XCloseDisplay)(self.display);
            }
        }
    }
}

// Safe to send across threads since X11 Display is thread-safe on Linux
unsafe impl Send for EmbeddedWindow {}
