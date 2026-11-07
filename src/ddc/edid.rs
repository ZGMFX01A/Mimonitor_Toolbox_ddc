//! Physical dimensions come from the exact Windows monitor interface's EDID,
//! never a product-name guess or another instance of the same monitor model.
use windows::core::PCWSTR;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_BINARY};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DisplaySize {
    pub width_mm: u16,
    pub height_mm: u16,
}

impl DisplaySize {
    pub fn diagonal_inches(self) -> f64 {
        (f64::from(self.width_mm).hypot(f64::from(self.height_mm)) / 25.4 * 10.0).round() / 10.0
    }
    pub fn label(self) -> String {
        format!("约 {:.1} 英寸", self.diagonal_inches())
    }
}

fn plausible(width: u16, height: u16) -> Option<DisplaySize> {
    (width >= 50 && height >= 50 && width <= 4000 && height <= 4000).then_some(DisplaySize {
        width_mm: width,
        height_mm: height,
    })
}

pub fn parse_edid_size(edid: &[u8]) -> Option<DisplaySize> {
    if edid.len() < 128
        || edid[..8] != [0, 255, 255, 255, 255, 255, 255, 0]
        || edid[..128]
            .iter()
            .fold(0u8, |sum, byte| sum.wrapping_add(*byte))
            != 0
    {
        return None;
    }
    // The preferred detailed timing carries millimetres; the basic block only
    // carries rounded centimetres. Other timing descriptors may refer to modes
    // with different active areas, so do not select one arbitrarily.
    let d = &edid[54..72];
    if d[0] != 0 || d[1] != 0 {
        let width = u16::from(d[12]) | (u16::from(d[14] & 0xf0) << 4);
        let height = u16::from(d[13]) | (u16::from(d[14] & 0x0f) << 8);
        if let Some(size) = plausible(width, height) {
            return Some(size);
        }
    }
    // If just one dimension is zero, EDID 1.4 encodes an aspect ratio, not size.
    if edid[21] == 0 || edid[22] == 0 {
        return None;
    }
    plausible(u16::from(edid[21]) * 10, u16::from(edid[22]) * 10)
}

fn registry_path(interface: &str) -> Option<String> {
    let parts: Vec<_> = interface.strip_prefix(r"\\?\")?.split('#').collect();
    if parts.len() != 4 || !parts[0].eq_ignore_ascii_case("DISPLAY") {
        return None;
    }
    // Reject separators rather than allow fallback enumeration across devices.
    if parts[1..3]
        .iter()
        .any(|part| part.is_empty() || part.contains(['\\', '/', '\0']))
    {
        return None;
    }
    Some(format!(
        r"SYSTEM\CurrentControlSet\Enum\DISPLAY\{}\{}\Device Parameters",
        parts[1], parts[2]
    ))
}

pub fn read_display_size(interface: &str) -> Option<DisplaySize> {
    let path: Vec<u16> = registry_path(interface)?
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let value: Vec<u16> = "EDID\0".encode_utf16().collect();
    let mut data = [0u8; 4096];
    let mut len = data.len() as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(path.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_BINARY,
            None,
            Some(data.as_mut_ptr().cast()),
            Some(&mut len),
        )
    };
    if result.0 != 0 || len as usize > data.len() {
        return None;
    }
    parse_edid_size(&data[..len as usize])
}

#[cfg(test)]
mod tests {
    use super::*;
    fn valid(mut b: [u8; 128]) -> [u8; 128] {
        b[..8].copy_from_slice(&[0, 255, 255, 255, 255, 255, 255, 0]);
        b[127] = 0u8.wrapping_sub(b[..127].iter().fold(0u8, |s, b| s.wrapping_add(*b)));
        b
    }
    #[test]
    fn precise_size_and_centimetre_fallback() {
        let mut b = [0; 128];
        b[21] = 70;
        b[22] = 39;
        b[54] = 1;
        b[66] = 186;
        b[67] = 137;
        b[68] = 0x21;
        assert_eq!(parse_edid_size(&valid(b)).unwrap().diagonal_inches(), 31.5);
        b[54] = 0;
        assert_eq!(
            parse_edid_size(&valid(b)),
            Some(DisplaySize {
                width_mm: 700,
                height_mm: 390
            })
        );
    }
    #[test]
    fn rejects_corrupt_unknown_and_aspect_ratio() {
        let mut b = [0; 128];
        b[21] = 35;
        b[22] = 20;
        let mut b = valid(b);
        b[24] = 1;
        assert!(parse_edid_size(&b).is_none());
        b[21] = 0;
        assert!(parse_edid_size(&valid(b)).is_none());
        assert!(parse_edid_size(&[0; 127]).is_none());
    }
    #[test]
    fn exact_instance_only() {
        let id = r"\\?\DISPLAY#XMI300A#5&123&0&UID4352#{GUID}";
        assert!(registry_path(id)
            .unwrap()
            .contains(r"XMI300A\5&123&0&UID4352\"));
        assert!(registry_path("XMI300A").is_none());
        assert!(registry_path(r"\\?\DISPLAY#XMI300A#..\other#{GUID}").is_none());
    }
}
