//! Which VP9 decoders the GStreamer that GTK plays Home's background through has, for
//! `pindu_core::home::has_hardware_decoder`.
//!
//! GTK's media backend loads and initialises GStreamer; Pindu does not link it. The probe
//! asks the copy GTK has loaded, through `dlopen` with `RTLD_NOLOAD`, so it never loads or
//! initialises GStreamer itself.

use std::ffi::{CStr, c_char, c_int, c_uint, c_void};

use gtk::glib::ffi::{GList, gboolean};

/// `dlopen`'s flag to resolve symbols as they are used.
const RTLD_LAZY: c_int = 0x1;
/// `dlopen`'s flag to open a library only if it is already loaded.
const RTLD_NOLOAD: c_int = 0x4;

/// `GST_ELEMENT_FACTORY_TYPE_DECODER`.
const FACTORY_TYPE_DECODER: u64 = 1 << 0;
/// `GST_ELEMENT_FACTORY_TYPE_MEDIA_VIDEO`.
const FACTORY_TYPE_MEDIA_VIDEO: u64 = 1 << 50;
/// `GST_RANK_MARGINAL`: the lowest rank `decodebin`, and so GTK's player, picks from.
const RANK_MARGINAL: c_uint = 64;
/// `GST_PAD_SINK`: the decoder's input.
const PAD_SINK: c_int = 2;

unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
}

type IsInitialized = unsafe extern "C" fn() -> gboolean;
type ListGetElements = unsafe extern "C" fn(u64, c_uint) -> *mut GList;
type CapsFromString = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type ListFilter = unsafe extern "C" fn(*mut GList, *const c_void, c_int, gboolean) -> *mut GList;
type GetMetadata = unsafe extern "C" fn(*mut c_void, *const c_char) -> *const c_char;
type ListFree = unsafe extern "C" fn(*mut GList);
type MiniObjectUnref = unsafe extern "C" fn(*mut c_void);

/// The functions of a loaded GStreamer the probe calls.
struct Gstreamer {
    is_initialized: IsInitialized,
    list_get_elements: ListGetElements,
    caps_from_string: CapsFromString,
    list_filter: ListFilter,
    get_metadata: GetMetadata,
    list_free: ListFree,
    mini_object_unref: MiniObjectUnref,
}

impl Gstreamer {
    /// Looks the functions up in `library`, or `None` if one is missing.
    ///
    /// # Safety
    ///
    /// `library` is a `dlopen` handle of GStreamer 1.x.
    unsafe fn load(library: *mut c_void) -> Option<Self> {
        // SAFETY: `library` is a `dlopen` handle, as the caller promises, and each type
        // is the C signature of the GStreamer 1.x function it is looked up for.
        unsafe {
            Some(Self {
                is_initialized: symbol(library, c"gst_is_initialized")?,
                list_get_elements: symbol(library, c"gst_element_factory_list_get_elements")?,
                caps_from_string: symbol(library, c"gst_caps_from_string")?,
                list_filter: symbol(library, c"gst_element_factory_list_filter")?,
                get_metadata: symbol(library, c"gst_element_factory_get_metadata")?,
                list_free: symbol(library, c"gst_plugin_feature_list_free")?,
                mini_object_unref: symbol(library, c"gst_mini_object_unref")?,
            })
        }
    }
}

/// The address of `name` in `library`, as a `T`.
///
/// # Safety
///
/// `library` is a handle from `dlopen`, and `T` is the function pointer type of `name`.
unsafe fn symbol<T: Copy>(library: *mut c_void, name: &CStr) -> Option<T> {
    // SAFETY: `library` is a `dlopen` handle, as the caller promises, and `name` a C string.
    let address = unsafe { dlsym(library, name.as_ptr()) };
    // SAFETY: the caller promises `T` is the type of the function at `address`, and a
    // function pointer has the size of a data pointer on the platforms GStreamer runs on.
    (!address.is_null()).then(|| unsafe { std::mem::transmute_copy::<*mut c_void, T>(&address) })
}

/// The classes (`Codec/Decoder/Video/Hardware` and the like) of the video decoders that
/// take VP9 and that GTK's player may pick, or `None` when no initialised GStreamer is
/// loaded: GTK plays media some other way, or has not opened any yet.
pub(crate) fn vp9_decoder_classes() -> Option<Vec<String>> {
    // SAFETY: the name is a C string; `RTLD_NOLOAD` only returns a library already loaded.
    let library = unsafe { dlopen(c"libgstreamer-1.0.so.0".as_ptr(), RTLD_LAZY | RTLD_NOLOAD) };
    if library.is_null() {
        return None;
    }
    // SAFETY: `library` is the handle just opened, of the GStreamer 1.x GTK loaded.
    let gstreamer = unsafe { Gstreamer::load(library) };
    let classes = gstreamer.and_then(|gstreamer| decoder_classes(&gstreamer));
    // SAFETY: `library` came from `dlopen`, and nothing from it is used after this.
    unsafe { dlclose(library) };
    classes
}

/// The classes of the VP9 video decoders `gstreamer` has, once it is initialised.
fn decoder_classes(gstreamer: &Gstreamer) -> Option<Vec<String>> {
    // SAFETY: `gst_is_initialized` takes nothing and may be called at any time.
    if unsafe { (gstreamer.is_initialized)() } == 0 {
        return None;
    }
    // SAFETY: GStreamer is initialised. `decoders` and `vp9` are a new list and new caps,
    // which `gst_element_factory_list_filter` only reads and which are freed once; the
    // list it returns holds its own references, freed after its classes are copied. Each
    // `data` is a `GstElementFactory`, whose metadata is NULL or a C string it owns.
    unsafe {
        let decoders = (gstreamer.list_get_elements)(
            FACTORY_TYPE_DECODER | FACTORY_TYPE_MEDIA_VIDEO,
            RANK_MARGINAL,
        );
        let vp9 = (gstreamer.caps_from_string)(c"video/x-vp9".as_ptr());
        if vp9.is_null() {
            (gstreamer.list_free)(decoders);
            return None;
        }
        let filtered = (gstreamer.list_filter)(decoders, vp9.cast_const(), PAD_SINK, 0);
        (gstreamer.list_free)(decoders);
        (gstreamer.mini_object_unref)(vp9);
        let mut classes = Vec::new();
        let mut item = filtered;
        while !item.is_null() {
            let class = (gstreamer.get_metadata)((*item).data, c"klass".as_ptr());
            if !class.is_null() {
                classes.push(CStr::from_ptr(class).to_string_lossy().into_owned());
            }
            item = (*item).next;
        }
        (gstreamer.list_free)(filtered);
        Some(classes)
    }
}
