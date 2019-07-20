// Copyright (C) 2016  ParadoxSpiral
//
// This file is part of mpv-rs.
//
// This library is free software; you can redistribute it and/or
// modify it under the terms of the GNU Lesser General Public
// License as published by the Free Software Foundation; either
// version 2.1 of the License, or (at your option) any later version.
//
// This library is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
// Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public
// License along with this library; if not, write to the Free Software
// Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA  02110-1301  USA

mod errors {
    use std::ffi::NulError;
    use std::os::raw as ctype;
    use std::rc::Rc;
    use std::str::Utf8Error;

    #[allow(missing_docs)]
    pub type Result<T> = ::std::result::Result<T, Error>;

    #[allow(missing_docs)]
    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    pub enum Error {
        Loadfiles {
            index: usize,
            error: Rc<Error>,
        },
        VersionMismatch {
            linked: ctype::c_ulong,
            loaded: ctype::c_ulong,
        },
        InvalidUtf8,
        Null,
        Raw(crate::MpvError),
    }

    impl From<NulError> for Error {
        fn from(_other: NulError) -> Error {
            Error::Null
        }
    }
    impl From<Utf8Error> for Error {
        fn from(_other: Utf8Error) -> Error {
            Error::InvalidUtf8
        }
    }
    impl From<crate::MpvError> for Error {
        fn from(other: crate::MpvError) -> Error {
            Error::Raw(other)
        }
    }
}

pub use self::errors::*;

#[cfg(unix)]
macro_rules! mpv_cstr_to_str {
    ($cstr: expr) => {{
        use std::{
            ffi::{CStr, OsStr},
            os::unix::ffi::OsStrExt,
        };
        if let Some(v) = OsStr::from_bytes(CStr::from_ptr($cstr).to_bytes()).to_str() {
            // Not sure why the type isn't inferred
            let r: Result<&str> = Ok(v);
            r
        } else {
            Err(Error::InvalidUtf8)
        }
    }};
}

#[cfg(not(unix))]
macro_rules! mpv_cstr_to_str {
    ($cstr: expr) => {
        if let Ok(v) = std::str::from_utf8(std::ffi::CStr::from_ptr($cstr).to_bytes()) {
            // Not sure why the type isn't inferred
            let r: Result<&str> = Ok(v);
            r
        } else {
            Err(Error::InvalidUtf8)
        }
    };
}

/// Event handling
pub mod events;
#[cfg(feature = "protocols")]
/// Custom protocols
pub mod protocol;
#[cfg(feature = "render")]
/// Custom rendering
pub mod render;

use super::*;

#[cfg(feature = "events_sync")]
use parking_lot::{self, Mutex};

use std::ffi::CString;
use std::mem::MaybeUninit;
use std::ops::Deref;
use std::os::raw as ctype;
use std::ptr::{self, NonNull};
#[cfg(feature = "protocols")]
use std::sync::atomic::AtomicBool;

fn mpv_err<T>(ret: T, err: ctype::c_int) -> Result<T> {
    if err == 0 {
        Ok(ret)
    } else {
        Err(Error::Raw(err))
    }
}

#[allow(missing_docs)]
/// This trait describes which types are allowed to be passed to getter mpv APIs.
pub unsafe trait GetData: Sized {
    #[doc(hidden)]
    fn get_from_c_void<T, F: FnMut(*mut ctype::c_void) -> Result<T>>(mut fun: F) -> Result<Self> {
        let mut val = MaybeUninit::uninit();
        let _ = fun(val.as_mut_ptr() as *mut _)?;
        Ok(unsafe { val.assume_init() })
    }
    fn get_format() -> Format;
}

#[allow(missing_docs)]
/// This trait describes which types are allowed to be passed to setter mpv APIs.
pub unsafe trait SetData: Sized {
    #[doc(hidden)]
    fn call_as_c_void<T, F: FnMut(*mut ctype::c_void) -> Result<T>>(
        mut self,
        mut fun: F,
    ) -> Result<T> {
        fun(&mut self as *mut Self as _)
    }
    fn get_format() -> Format;
}

unsafe impl GetData for f64 {
    #[inline]
    fn get_format() -> Format {
        Format::Double
    }
}

unsafe impl SetData for f64 {
    #[inline]
    fn get_format() -> Format {
        Format::Double
    }
}

unsafe impl GetData for i64 {
    #[inline]
    fn get_format() -> Format {
        Format::Int64
    }
}

unsafe impl SetData for i64 {
    #[inline]
    fn get_format() -> Format {
        Format::Int64
    }
}

unsafe impl GetData for bool {
    #[inline]
    fn get_from_c_void<T, F: FnMut(*mut ctype::c_void) -> Result<T>>(mut fun: F) -> Result<bool> {
        let mut val = MaybeUninit::uninit();
        let _ = fun(val.as_mut_ptr() as *mut _)?;
        Ok(unsafe { val.assume_init() })
    }

    #[inline]
    fn get_format() -> Format {
        Format::Flag
    }
}

unsafe impl SetData for bool {
    #[inline]
    fn call_as_c_void<T, F: FnMut(*mut ctype::c_void) -> Result<T>>(self, mut fun: F) -> Result<T> {
        let mut cpy: i64 = if self { 1 } else { 0 };
        fun(&mut cpy as *mut i64 as *mut _)
    }

    #[inline]
    fn get_format() -> Format {
        Format::Flag
    }
}

unsafe impl GetData for String {
    #[inline]
    fn get_from_c_void<T, F: FnMut(*mut ctype::c_void) -> Result<T>>(mut fun: F) -> Result<String> {
        let ptr = &mut ptr::null();
        let _ = fun(ptr as *mut *const ctype::c_char as _)?;

        let ret = unsafe { mpv_cstr_to_str!(*ptr) }?.to_owned();
        unsafe { mpv_sys::mpv_free(*ptr as *mut _) };
        Ok(ret)
    }

    #[inline]
    fn get_format() -> Format {
        Format::String
    }
}

unsafe impl SetData for String {
    #[inline]
    fn call_as_c_void<T, F: FnMut(*mut ctype::c_void) -> Result<T>>(self, mut fun: F) -> Result<T> {
        let string = CString::new(self)?;
        fun((&mut string.as_ptr()) as *mut *const ctype::c_char as *mut _)
    }

    #[inline]
    fn get_format() -> Format {
        Format::String
    }
}

/// Wrapper around an `&str` returned by mpv, that properly deallocates it with mpv's allocator.
pub struct MpvStr<'a>(&'a str);
impl<'a> Deref for MpvStr<'a> {
    type Target = str;

    fn deref(&self) -> &str {
        self.0
    }
}
impl<'a> Drop for MpvStr<'a> {
    fn drop(&mut self) {
        unsafe { mpv_sys::mpv_free(self.0.as_ptr() as *mut u8 as _) };
    }
}

unsafe impl<'a> GetData for MpvStr<'a> {
    #[inline]
    fn get_from_c_void<T, F: FnMut(*mut ctype::c_void) -> Result<T>>(
        mut fun: F,
    ) -> Result<MpvStr<'a>> {
        let ptr = &mut ptr::null();
        let _ = fun(ptr as *mut *const ctype::c_char as _)?;

        Ok(MpvStr(unsafe { mpv_cstr_to_str!(*ptr) }?))
    }

    #[inline]
    fn get_format() -> Format {
        Format::String
    }
}

unsafe impl<'a> SetData for &'a str {
    #[inline]
    fn call_as_c_void<T, F: FnMut(*mut ctype::c_void) -> Result<T>>(self, mut fun: F) -> Result<T> {
        let string = CString::new(self)?;
        fun((&mut string.as_ptr()) as *mut *const ctype::c_char as *mut _)
    }

    #[inline]
    fn get_format() -> Format {
        Format::String
    }
}

#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
/// Subset of `mpv_format` used by the public API.
pub enum Format {
    String,
    Flag,
    Int64,
    Double,
}

impl Format {
    fn as_mpv_format(&self) -> MpvFormat {
        match *self {
            Format::String => mpv_format::String,
            Format::Flag => mpv_format::Flag,
            Format::Int64 => mpv_format::Int64,
            Format::Double => mpv_format::Double,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// How a `File` is inserted into the playlist.
pub enum FileState {
    /// Replace the current track.
    Replace,
    /// Append to the current playlist.
    Append,
    /// If current playlist is empty: play, otherwise append to playlist.
    AppendPlay,
}

impl FileState {
    fn val(&self) -> &str {
        match *self {
            FileState::Replace => "replace",
            FileState::Append => "append",
            FileState::AppendPlay => "append-play",
        }
    }
}

pub struct Mpv {
    /// The handle to the mpv core
    pub ctx: NonNull<mpv_sys::mpv_handle>,
    #[cfg(feature = "events_sync")]
    ev_iter_notification: Box<(Mutex<bool>, parking_lot::Condvar)>,
    #[cfg(feature = "events_sync")]
    ev_to_observe: Mutex<Vec<events::sync::Event>>,
    #[cfg(feature = "events_sync")]
    ev_to_observe_properties: Mutex<::std::collections::HashMap<String, u64>>,
    #[cfg(feature = "events_sync")]
    ev_observed: Mutex<Vec<events::sync::Event>>,
    #[cfg(feature = "protocols")]
    protocols_guard: AtomicBool,
}

unsafe impl Send for Mpv {}
unsafe impl Sync for Mpv {}

impl Drop for Mpv {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            mpv_sys::mpv_terminate_destroy(self.ctx.as_ptr());
        }
    }
}

impl Mpv {
    #[cfg(not(feature = "events_sync"))]
    #[inline]
    /// Create a new `Mpv`.
    /// The default settings can be probed by running: `$ mpv --show-profile=libmpv`
    pub fn new() -> Result<Mpv> {
        let api_version = unsafe { mpv_sys::mpv_client_api_version() };
        if crate::MPV_CLIENT_API_VERSION != api_version {
            return Err(Error::VersionMismatch {
                linked: crate::MPV_CLIENT_API_VERSION,
                loaded: api_version,
            });
        }

        let ctx = unsafe { mpv_sys::mpv_create() };
        if ctx.is_null() {
            return Err(Error::Null);
        }
        mpv_err((), unsafe { mpv_sys::mpv_initialize(ctx) }).or_else(|err| {
            unsafe { mpv_sys::mpv_terminate_destroy(ctx) };
            Err(err)
        })?;

        Ok(Mpv {
            ctx: unsafe { NonNull::new_unchecked(ctx) },
            #[cfg(feature = "protocols")]
            protocols_guard: AtomicBool::new(false),
        })
    }

    #[inline]
    /// Load a configuration file. The path has to be absolute, and a file.
    pub fn load_config(&self, path: &str) -> Result<()> {
        let file = CString::new(path)?.into_raw();
        let ret = mpv_err((), unsafe {
            mpv_sys::mpv_load_config_file(self.ctx.as_ptr(), file)
        });
        unsafe { CString::from_raw(file) };
        ret
    }

    #[inline]
    /// Send a command to the `Mpv` instance. This uses `mpv_command_string` internally,
    /// so that the syntax is the same as described in the [manual for the input.conf]
    /// (https://mpv.io/manual/master/#list-of-input-commands).
    ///
    /// Note that you may have to escape strings with `""` when they contain spaces.
    pub fn command(&self, name: &str, args: &[&str]) -> Result<()> {
        let mut cmd =
            String::with_capacity(name.len() + args.iter().fold(0, |acc, e| acc + e.len() + 1));
        cmd.push_str(name);

        for elem in args {
            cmd.push_str(" ");
            cmd.push_str(elem);
        }
        let raw = CString::new(cmd)?;

        mpv_err((), unsafe {
            mpv_sys::mpv_command_string(self.ctx.as_ptr(), raw.as_ptr())
        })
    }

    #[inline]
    /// Set the value of a property.
    pub fn set_property<T: SetData>(&self, name: &str, data: T) -> Result<()> {
        let name = CString::new(name)?;
        let format = T::get_format().as_mpv_format() as _;
        data.call_as_c_void(|ptr| {
            mpv_err((), unsafe {
                mpv_sys::mpv_set_property(self.ctx.as_ptr(), name.as_ptr(), format, ptr)
            })
        })
    }

    #[inline]
    /// Get the value of a property.
    pub fn get_property<T: GetData>(&self, name: &str) -> Result<T> {
        let name = CString::new(name)?;

        let format = T::get_format().as_mpv_format() as _;
        T::get_from_c_void(|ptr| {
            mpv_err((), unsafe {
                mpv_sys::mpv_get_property(self.ctx.as_ptr(), name.as_ptr(), format, ptr)
            })
        })
    }

    #[inline]
    /// Internal time in microseconds, this has an arbitrary offset, and will never go backwards.
    ///
    /// This can be called at any time, even if it was stated that no API function should be called.
    pub fn get_internal_time(&self) -> i64 {
        unsafe { mpv_sys::mpv_get_time_us(self.ctx.as_ptr()) }
    }

    // --- Convenience property functions ---
    //

    #[inline]
    /// Add -or subtract- any value from a property. Over/underflow clamps to max/min.
    pub fn add_property(&self, property: &str, value: isize) -> Result<()> {
        self.command("add", &[property, &format!("{}", value)])
    }

    #[inline]
    /// Cycle through a given property. `up` specifies direction. On
    /// overflow, set the property back to the minimum, on underflow set it to the maximum.
    pub fn cycle_property(&self, property: &str, up: bool) -> Result<()> {
        self.command("cycle", &[property, if up { "up" } else { "down" }])
    }

    #[inline]
    /// Multiply any property with any positive factor.
    pub fn multiply_property(&self, property: &str, factor: usize) -> Result<()> {
        self.command("multiply", &[property, &format!("{}", factor)])
    }

    #[inline]
    /// Pause playback at runtime.
    pub fn pause(&self) -> Result<()> {
        self.set_property("pause", true)
    }

    #[inline]
    /// Unpause playback at runtime.
    pub fn unpause(&self) -> Result<()> {
        self.set_property("pause", false)
    }

    // --- Convenience command functions ---
    //

    #[inline]
    #[cfg(any(feature = "events_simple", feature = "events_complex"))]
    /// Enable an event.
    pub fn enable_event(&self, ev: events::EventId) -> Result<()> {
        mpv_err((), unsafe {
            mpv_sys::mpv_request_event(self.ctx.as_ptr(), ev, 1)
        })
    }

    #[inline]
    #[cfg(any(feature = "events_simple", feature = "events_complex"))]
    /// Enable all, except deprecated, events.
    pub fn enable_all_events(&self) -> Result<()> {
        for i in (1..9)
            .chain(11..12)
            .chain(14..15)
            .chain(16..19)
            .chain(20..23)
            .chain(23..26)
        {
            self.enable_event(i)?;
        }
        Ok(())
    }

    #[inline]
    #[cfg(any(feature = "events_simple", feature = "events_complex"))]
    /// Disable an event.
    pub fn disable_event(&self, ev: events::EventId) -> Result<()> {
        mpv_err((), unsafe {
            mpv_sys::mpv_request_event(self.ctx.as_ptr(), ev, 0)
        })
    }

    #[inline]
    #[cfg(any(feature = "events_simple", feature = "events_complex"))]
    /// Diable all deprecated events.
    pub fn disable_deprecated_events(&self) -> Result<()> {
        self.disable_event(mpv_sys::mpv_event_id_MPV_EVENT_TRACKS_CHANGED)?;
        self.disable_event(mpv_sys::mpv_event_id_MPV_EVENT_TRACK_SWITCHED)?;
        self.disable_event(mpv_sys::mpv_event_id_MPV_EVENT_PAUSE)?;
        self.disable_event(mpv_sys::mpv_event_id_MPV_EVENT_UNPAUSE)?;
        self.disable_event(mpv_sys::mpv_event_id_MPV_EVENT_SCRIPT_INPUT_DISPATCH)?;
        self.disable_event(mpv_sys::mpv_event_id_MPV_EVENT_METADATA_UPDATE)?;
        self.disable_event(mpv_sys::mpv_event_id_MPV_EVENT_CHAPTER_CHANGE)?;
        Ok(())
    }

    #[inline]
    #[cfg(any(feature = "events_simple", feature = "events_complex"))]
    /// Diable all events.
    pub fn disable_all_events(&self) -> Result<()> {
        for i in 1..26 {
            self.disable_event(i as _)?;
        }
        Ok(())
    }

    // --- Seek functions ---
    //

    #[inline]
    /// Seek forward relatively from current position in seconds.
    /// This is less exact than `seek_absolute`, see [mpv manual]
    /// (https://mpv.io/manual/master/#command-interface-
    /// [relative|absolute|absolute-percent|relative-percent|exact|keyframes]).
    pub fn seek_forward(&self, secs: ctype::c_double) -> Result<()> {
        self.command("seek", &[&format!("{}", secs), "relative"])
    }

    #[inline]
    /// See `seek_forward`.
    pub fn seek_backward(&self, secs: ctype::c_double) -> Result<()> {
        self.command("seek", &[&format!("-{}", secs), "relative"])
    }

    #[inline]
    /// Seek to a given absolute secs.
    pub fn seek_absolute(&self, secs: ctype::c_double) -> Result<()> {
        self.command("seek", &[&format!("{}", secs), "absolute"])
    }

    #[inline]
    /// Seek to a given relative percent position (may be negative).
    /// If `percent` of the playtime is bigger than the remaining playtime, the next file is played.
    /// out of bounds values are clamped to either 0 or 100.
    pub fn seek_percent(&self, percent: isize) -> Result<()> {
        self.command("seek", &[&format!("{}", percent), "relative-percent"])
    }

    #[inline]
    /// Seek to the given percentage of the playtime.
    pub fn seek_percent_absolute(&self, percent: usize) -> Result<()> {
        self.command("seek", &[&format!("{}", percent), "relative-percent"])
    }

    #[inline]
    /// Revert the previous `seek_` call, can also revert itself.
    pub fn seek_revert(&self) -> Result<()> {
        self.command("revert-seek", &[])
    }

    #[inline]
    /// Mark the current position as the position that will be seeked to by `seek_revert`.
    pub fn seek_revert_mark(&self) -> Result<()> {
        self.command("revert-seek", &["mark"])
    }

    #[inline]
    /// Seek exactly one frame, and pause.
    /// Noop on audio only streams.
    pub fn seek_frame(&self) -> Result<()> {
        self.command("frame-step", &[])
    }

    #[inline]
    /// See `seek_frame`.
    /// [Note performance considerations.](https://mpv.io/manual/master/#command-interface-frame-back-step)
    pub fn seek_frame_backward(&self) -> Result<()> {
        self.command("frame-back-step", &[])
    }

    // --- Screenshot functions ---
    //

    #[inline]
    /// "Save the video image, in its original resolution, and with subtitles.
    /// Some video outputs may still include the OSD in the output under certain circumstances.".
    ///
    /// "[O]ptionally save it to a given file. The format of the file will be
    /// guessed by the extension (and --screenshot-format is ignored - the behaviour when the
    /// extension is missing or unknown is arbitrary). If the file already exists, it's overwritten.
    /// Like all input command parameters, the filename is subject to property expansion as
    /// described in [Property Expansion](https://mpv.io/manual/master/#property-expansion)."
    pub fn screenshot_subtitles<'a, A: Into<Option<&'a str>>>(&self, path: A) -> Result<()> {
        if let Some(path) = path.into() {
            self.command("screenshot", &[&format!("\"{}\"", path), "subtitles"])
        } else {
            self.command("screenshot", &["subtitles"])
        }
    }

    #[inline]
    /// "Like subtitles, but typically without OSD or subtitles. The exact behavior
    /// depends on the selected video output."
    pub fn screenshot_video<'a, A: Into<Option<&'a str>>>(&self, path: A) -> Result<()> {
        if let Some(path) = path.into() {
            self.command("screenshot", &[&format!("\"{}\"", path), "video"])
        } else {
            self.command("screenshot", &["video"])
        }
    }

    #[inline]
    /// "Save the contents of the mpv window. Typically scaled, with OSD and subtitles. The exact
    /// behaviour depends on the selected video output, and if no support is available,
    /// this will act like video.".
    pub fn screenshot_window<'a, A: Into<Option<&'a str>>>(&self, path: A) -> Result<()> {
        if let Some(path) = path.into() {
            self.command("screenshot", &[&format!("\"{}\"", path), "window"])
        } else {
            self.command("screenshot", &["window"])
        }
    }

    // --- Playlist functions ---
    //

    #[inline]
    /// Play the next item of the current playlist.
    /// Does nothing if the current item is the last item.
    pub fn playlist_next_weak(&self) -> Result<()> {
        self.command("playlist-next", &["weak"])
    }

    #[inline]
    /// Play the next item of the current playlist.
    /// Terminates playback if the current item is the last item.
    pub fn playlist_next_force(&self) -> Result<()> {
        self.command("playlist-next", &["force"])
    }

    #[inline]
    /// See `playlist_next_weak`.
    pub fn playlist_previous_weak(&self) -> Result<()> {
        self.command("playlist-previous", &["weak"])
    }

    #[inline]
    /// See `playlist_next_force`.
    pub fn playlist_previous_force(&self) -> Result<()> {
        self.command("playlist-previous", &["force"])
    }

    #[inline]
    /// The given files are loaded sequentially, returning the index of the current file
    /// and the error in case of an error. [More information.](https://mpv.io/manual/master/#command-interface-[replace|append|append-play)
    ///
    /// # Arguments
    /// The `files` tuple slice consists of:
    ///     * a string slice - the path
    ///     * a `FileState` - how the file will be opened
    ///     * an optional string slice - any additional options that will be set for this file
    ///
    /// # Peculiarities
    /// `loadfile` is kind of asynchronous, any additional option is set during loading,
    /// [specifics](https://github.com/mpv-player/mpv/issues/4089).
    pub fn playlist_load_files<'a, A>(&self, files: &[(&str, FileState, A)]) -> Result<()>
    where
        A: Into<Option<&'a str>> + Clone,
    {
        for (i, elem) in files.iter().enumerate() {
            let args = elem.2.clone().into().unwrap_or("");

            let ret = self.command(
                "loadfile",
                &[&format!("\"{}\"", elem.0), elem.1.val(), args],
            );

            if ret.is_err() {
                return Err(Error::Loadfiles {
                    index: i,
                    error: ::std::rc::Rc::new(ret.unwrap_err()),
                });
            }
        }
        Ok(())
    }

    #[inline]
    /// Load the given playlist file, that either replaces the current playlist, or appends to it.
    pub fn playlist_load_list(&self, path: &str, replace: bool) -> Result<()> {
        if replace {
            self.command("loadlist", &[&format!("\"{}\"", path), "replace"])
        } else {
            self.command("loadlist", &[&format!("\"{}\"", path), "append"])
        }
    }

    #[inline]
    /// Remove every, except the current, item from the playlist.
    pub fn playlist_clear(&self) -> Result<()> {
        self.command("playlist-clear", &[])
    }

    #[inline]
    /// Remove the currently selected item from the playlist.
    pub fn playlist_remove_current(&self) -> Result<()> {
        self.command("playlist-remove", &["current"])
    }

    #[inline]
    /// Remove item at `position` from the playlist.
    pub fn playlist_remove_index(&self, position: usize) -> Result<()> {
        self.command("playlist-remove", &[&format!("{}", position)])
    }

    #[inline]
    /// Move item `old` to the position of item `new`.
    pub fn playlist_move(&self, old: usize, new: usize) -> Result<()> {
        self.command("playlist-move", &[&format!("{}", new), &format!("{}", old)])
    }

    #[inline]
    /// Shuffle the playlist.
    pub fn playlist_shuffle(&self) -> Result<()> {
        self.command("playlist-shuffle", &[])
    }

    // --- Subtitle functions ---
    //

    #[inline]
    /// Add and select the subtitle immediately.
    /// Specifying a language requires specifying a title.
    ///
    /// # Panics
    /// If a language but not title was specified.
    pub fn subtitle_add_select<'a, 'b, A: Into<Option<&'a str>>, B: Into<Option<&'b str>>>(
        &self,
        path: &str,
        title: A,
        lang: B,
    ) -> Result<()> {
        match (title.into(), lang.into()) {
            (None, None) => self.command("sub-add", &[&format!("\"{}\"", path), "select"]),
            (Some(t), None) => self.command("sub-add", &[&format!("\"{}\"", path), "select", t]),
            (None, Some(_)) => panic!("Given subtitle language, but missing title"),
            (Some(t), Some(l)) => {
                self.command("sub-add", &[&format!("\"{}\"", path), "select", t, l])
            }
        }
    }

    #[inline]
    /// See `AddSelect`. "Don't select the subtitle.
    /// (Or in some special situations, let the default stream selection mechanism decide.)".
    ///
    /// Returns an `Error::InvalidArgument` if a language, but not a title, was provided.
    ///
    /// # Panics
    /// If a language but not title was specified.
    pub fn subtitle_add_auto<'a, 'b, A: Into<Option<&'a str>>, B: Into<Option<&'b str>>>(
        &self,
        path: &str,
        title: A,
        lang: B,
    ) -> Result<()> {
        match (title.into(), lang.into()) {
            (None, None) => self.command("sub-add", &[&format!("\"{}\"", path), "auto"]),
            (Some(t), None) => self.command("sub-add", &[&format!("\"{}\"", path), "auto", t]),
            (Some(t), Some(l)) => {
                self.command("sub-add", &[&format!("\"{}\"", path), "auto", t, l])
            }
            (None, Some(_)) => panic!("Given subtitle language, but missing title"),
        }
    }

    #[inline]
    /// See `AddSelect`. "Select the subtitle. If a subtitle with the same file name was
    /// already added, that one is selected, instead of loading a duplicate entry.
    /// (In this case, title/language are ignored, and if the [sub] was changed since it was loaded,
    /// these changes won't be reflected.)".
    pub fn subtitle_add_cached(&self, path: &str) -> Result<()> {
        self.command("sub-add", &[&format!("\"{}\"", path), "cached"])
    }

    #[inline]
    /// "Remove the given subtitle track. If the id argument is missing, remove the current
    /// track. (Works on external subtitle files only.)"
    pub fn subtitle_remove<A: Into<Option<usize>>>(&self, index: A) -> Result<()> {
        if let Some(idx) = index.into() {
            self.command("sub-remove", &[&format!("{}", idx)])
        } else {
            self.command("sub-remove", &[])
        }
    }

    #[inline]
    /// "Reload the given subtitle track. If the id argument is missing, reload the current
    /// track. (Works on external subtitle files only.)"
    pub fn subtitle_reload<A: Into<Option<usize>>>(&self, index: A) -> Result<()> {
        if let Some(idx) = index.into() {
            self.command("sub-reload", &[&format!("{}", idx)])
        } else {
            self.command("sub-reload", &[])
        }
    }

    #[inline]
    /// "Change subtitle timing such, that the subtitle event after the next `isize` subtitle
    /// events is displayed. `isize` can be negative to step backwards."
    pub fn subtitle_step(&self, skip: isize) -> Result<()> {
        self.command("sub-step", &[&format!("{}", skip)])
    }

    #[inline]
    /// "Seek to the next subtitle. This is similar to sub-step, except that it seeks video and
    /// audio instead of adjusting the subtitle delay.
    /// For embedded subtitles (like with matroska), this works only with subtitle events that
    /// have already been displayed, or are within a short prefetch range."
    pub fn subtitle_seek_forward(&self) -> Result<()> {
        self.command("sub-seek", &["1"])
    }

    #[inline]
    /// See `SeekForward`.
    pub fn subtitle_seek_backward(&self) -> Result<()> {
        self.command("sub-seek", &["-1"])
    }
}
