.pragma library

// Single-colour device glyphs in the spirit of SF Symbols (macOS).
//
// Every outline is drawn on the same 24x24 grid and sits inside roughly
// 2.5..21.5 on both axes, matching macOS SF Symbols optical sizing and
// stroke weight (2.0) across all storage devices. Shapes use smooth corner
// radii and clean faceplate geometry, ensuring razor-sharp clarity at 20px.

// System / root volume ("system-drive" / Macintosh HD):
// Horizontal enclosure with rounded corners, lower drive tray seam,
// status LED dot, eject/optical slot, and upper system processor accent.
var systemDrive =
    "M6 5.5h12a2.5 2.5 0 0 1 2.5 2.5v8a2.5 2.5 0 0 1-2.5 2.5H6a2.5 2.5 0 0 1-2.5-2.5v-8A2.5 2.5 0 0 1 6 5.5z" +
    "M3.5 14h17" +
    "M6.5 16.3h5" +
    "M17.5 16.3h.01" +
    "M9.5 9.75h5"

// Internal disk ("hard-drive"):
// Clean horizontal internal drive sled with tray seam, activity LED, and slot.
var hardDrive =
    "M6 5.5h12a2.5 2.5 0 0 1 2.5 2.5v8a2.5 2.5 0 0 1-2.5 2.5H6a2.5 2.5 0 0 1-2.5-2.5v-8A2.5 2.5 0 0 1 6 5.5z" +
    "M3.5 14h17" +
    "M6.5 16.3h5" +
    "M17.5 16.3h.01"

// External disk ("external-drive"):
// Sleek portable external drive enclosure with USB-C connector collar at top,
// lower faceplate division, and centered activity LED.
var externalDrive =
    "M6.5 7.5h11a2.5 2.5 0 0 1 2.5 2.5v7a2.5 2.5 0 0 1-2.5 2.5h-11a2.5 2.5 0 0 1-2.5-2.5v-7A2.5 2.5 0 0 1 6.5 7.5z" +
    "M12 7.5V4" +
    "M10 4h4" +
    "M4 15h16" +
    "M12 17.2h.01"

// USB flash drive ("usb"):
// USB-A metal connector plug with dual contact pins, seated in a rounded
// housing with grip line and bottom lanyard notch.
var usb =
    "M9 6.5V3.5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v3" +
    "M10.5 4.2h.01M13.5 4.2h.01" +
    "M9 6.5h6a2.5 2.5 0 0 1 2.5 2.5v9a2.5 2.5 0 0 1-2.5 2.5H9a2.5 2.5 0 0 1-2.5-2.5v-9A2.5 2.5 0 0 1 9 6.5z" +
    "M9 11.5h6" +
    "M10.5 18h3"

// Smartphone ("smartphone"):
// Modern iPhone silhouette with Dynamic Island / speaker slot and home bar.
var smartphone =
    "M10 2.5h4a2.5 2.5 0 0 1 2.5 2.5v14a2.5 2.5 0 0 1-2.5 2.5h-4a2.5 2.5 0 0 1-2.5-2.5v-14A2.5 2.5 0 0 1 10 2.5z" +
    "M11 5h2" +
    "M10.5 19h3"

// Network volume ("network"):
// macOS Finder network globe with equator, meridian and latitude parallels.
var network =
    "M21.5 12a9.5 9.5 0 1 1-19 0 9.5 9.5 0 1 1 19 0" +
    "M2.5 12h19" +
    "M12 2.5c2.6 2.5 3.8 5.7 3.8 9.5s-1.2 7-3.8 9.5c-2.6-2.5-3.8-5.7-3.8-9.5s1.2-7 3.8-9.5" +
    "M4.5 7.5h15" +
    "M4.5 16.5h15"

// Optical disc ("disc"):
// CD/DVD disc with outer edge, spindle hole, and inner mirror clamp ring.
var disc =
    "M21.5 12a9.5 9.5 0 1 1-19 0 9.5 9.5 0 1 1 19 0" +
    "M14.5 12a2.5 2.5 0 1 1-5 0 2.5 2.5 0 1 1 5 0" +
    "M16.5 12a4.5 4.5 0 1 1-9 0 4.5 4.5 0 1 1 9 0"

// Camera ("camera"):
// Camera body, viewfinder prism, lens aperture, and flash dot.
var camera =
    "M8.5 5.5h7a1.5 1.5 0 0 1 1.3.8l.8 1.7h1.9a2.5 2.5 0 0 1 2.5 2.5v6.5a2.5 2.5 0 0 1-2.5 2.5h-15a2.5 2.5 0 0 1-2.5-2.5v-6.5a2.5 2.5 0 0 1 2.5-2.5h1.9l.8-1.7a1.5 1.5 0 0 1 1.3-.8z" +
    "M15 13a3 3 0 1 1-6 0 3 3 0 1 1 6 0" +
    "M18 10h.01"

// Encrypted volume ("encrypted-drive"):
// Drive enclosure with integrated padlock shackle and key slot.
var encryptedDrive =
    "M6 8.5h12a2.5 2.5 0 0 1 2.5 2.5v6a2.5 2.5 0 0 1-2.5 2.5H6a2.5 2.5 0 0 1-2.5-2.5v-6A2.5 2.5 0 0 1 6 8.5z" +
    "M3.5 15.5h17" +
    "M17.5 17.5h.01" +
    "M9 8.5V6a3 3 0 0 1 6 0v2.5" +
    "M12 11.5v2"

function pathFor(name) {
    switch (name) {
    case "system-drive":
        return systemDrive
    case "external-drive":
        return externalDrive
    case "usb":
        return usb
    case "smartphone":
    case "phone":
        return smartphone
    case "network":
        return network
    case "disc":
    case "optical-drive":
        return disc
    case "camera":
        return camera
    case "encrypted-drive":
        return encryptedDrive
    case "hard-drive":
    case "drive":
    default:
        return hardDrive
    }
}
