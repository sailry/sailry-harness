//! sysinfo has no GPU sampler. Read macOS IOAccelerator statistics through IOKit.
use objc2_core_foundation::{
    CFDictionary, CFNumber, CFRetained, CFString, CFType, kCFAllocatorDefault,
};
use objc2_io_kit::{
    IOIteratorNext, IOObjectRelease, IORegistryEntryCreateCFProperty,
    IORegistryEntryGetRegistryEntryID, IOServiceGetMatchingServices, IOServiceMatching,
};
use sailry_protocol::host::metrics::Gpu;

struct Object(u32);
impl Drop for Object {
    fn drop(&mut self) {
        IOObjectRelease(self.0);
    }
}

pub(super) fn sample() -> Vec<Gpu> {
    // SAFETY: the class name is a static, terminated C string; matching returns a CF dictionary.
    let Some(matching) = (unsafe { IOServiceMatching(c"IOAccelerator".as_ptr()) }) else {
        return Vec::new();
    };
    let mut iterator = 0;
    // SAFETY: matching is a retained CF dictionary and iterator is a writable output.
    // IOKit consumes the supplied dictionary reference and returns an owned iterator.
    if unsafe { IOServiceGetMatchingServices(0, Some(CFRetained::from(&matching)), &mut iterator) }
        != 0
    {
        return Vec::new();
    }
    let iterator = Object(iterator);
    let mut devices = Vec::new();
    loop {
        let entry = IOIteratorNext(iterator.0);
        if entry == 0 {
            break;
        }
        let entry = Object(entry);
        let mut id = 0;
        // SAFETY: entry retains the registry handle and id is a writable u64 output.
        if unsafe { IORegistryEntryGetRegistryEntryID(entry.0, &mut id) } != 0 {
            continue;
        }
        let name = property(entry.0, "model")
            .and_then(|value| value.downcast::<CFString>().ok())
            .map(|value| value.to_string())
            .unwrap_or_else(|| "GPU".into());
        let usage = property(entry.0, "PerformanceStatistics")
            .and_then(|value| value.downcast::<CFDictionary>().ok())
            .and_then(|values| {
                let key = CFString::from_static_str("Device Utilization %");
                // SAFETY: IOKit property dictionaries contain CF objects; key is a live CFString.
                let raw = unsafe { values.value((&*key as *const CFString).cast()) };
                if raw.is_null() {
                    return None;
                }
                // SAFETY: The dictionary owns this value throughout the type check and read.
                let value = unsafe { &*raw.cast::<CFType>() };
                value.downcast_ref::<CFNumber>()?.as_i64()
            })
            .filter(|value| (0..=100).contains(value))
            .map(|value| value as u32 * 100);
        devices.push(Gpu {
            id: id.to_string(),
            name,
            usage_basis_points: usage,
        });
        if devices.len() == 32 {
            break;
        }
    }
    devices.sort_by(|left, right| left.id.cmp(&right.id));
    devices
}

fn property(entry: u32, key: &'static str) -> Option<CFRetained<CFType>> {
    let key = CFString::from_static_str(key);
    // SAFETY: the caller retains entry and key is live; the default allocator
    // and zero options are the documented IOKit configuration.
    // The Create API returns a retained property; CFRetained releases it.
    unsafe { IORegistryEntryCreateCFProperty(entry, Some(&key), kCFAllocatorDefault, 0) }
}
