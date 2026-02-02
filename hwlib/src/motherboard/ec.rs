use crate::driver::PawnModuleManager;

/// Simple EC / IsaBridge diagnostic helpers.
///
/// Tries to use the `IsaBridgeEC` Pawn module (if available) to find and access SuperIO MMIO mapping.
/// Returns human-readable diagnostic lines.
pub fn diagnose_ec(pm: &mut PawnModuleManager) -> Vec<String> {
    let mut out = Vec::new();
    out.push("Starting EC diagnostic (IsaBridgeEC)".to_string());

    // 1) Try to call ioctl_find_superio_mmio and log hex values
    match pm.call_function("IsaBridgeEC", "ioctl_find_superio_mmio", &[]) {
        Ok(vec) => {
            let hexs: Vec<String> = vec.iter().map(|v| format!("0x{:X}", v)).collect();
            out.push(format!(
                "ioctl_find_superio_mmio returned {} values: {:?}",
                vec.len(),
                hexs
            ));
            if vec.len() >= 6 {
                let f_base = vec[0];
                let f_size = vec[1];
                let f_chip = vec[2];
                let s_base = vec[3];
                let s_size = vec[4];
                let s_chip = vec[5];
                out.push(format!(
                    "First MMIO: base=0x{:X}, size=0x{:X}, chip=0x{:X}",
                    f_base, f_size, f_chip
                ));
                out.push(format!(
                    "Second MMIO: base=0x{:X}, size=0x{:X}, chip=0x{:X}",
                    s_base, s_size, s_chip
                ));
            }
        }
        Err(e) => out.push(format!("ioctl_find_superio_mmio failed: {}", e)),
    }

    // 2) Try map and capture returned handles (if any), log hex
    let mut map_handles: Vec<u64> = Vec::new();
    match pm.call_function("IsaBridgeEC", "ioctl_map_superio_mmio", &[]) {
        Ok(vec) => {
            let hexs: Vec<String> = vec.iter().map(|v| format!("0x{:X}", v)).collect();
            out.push(format!(
                "ioctl_map_superio_mmio returned {} values: {:?}",
                vec.len(),
                hexs
            ));
            if !vec.is_empty() {
                map_handles = vec;
            }
        }
        Err(e) => out.push(format!("ioctl_map_superio_mmio failed: {}", e)),
    }

    // 3) Try reading a few offsets using several candidate handles/indices
    let offsets = [0u64, 1, 4, 0x20, 0x21, 0xA0];
    // Candidate first-parameters to try: returned handles + 0 and 1
    let mut candidates: Vec<u64> = map_handles.clone();
    candidates.push(0);
    candidates.push(1);

    for &handle in &candidates {
        out.push(format!(
            "-- Testing access with first-param handle 0x{:X}",
            handle
        ));
        for &offset in &offsets {
            // params: firstParam (maybe mapping handle or index), offset, size, is_write (0), value
            let params = [handle, offset, 1u64, 0u64, 0u64];
            match pm.call_function("IsaBridgeEC", "ioctl_access_superio_mmio", &params) {
                Ok(v) if !v.is_empty() => {
                    let hexs: Vec<String> = v.iter().map(|x| format!("0x{:X}", x)).collect();
                    out.push(format!(
                        "ioctl_access_superio_mmio handle 0x{:X} read offset 0x{:X} => {:?}",
                        handle, offset, hexs
                    ));
                }
                Ok(_) => out.push(format!(
                    "ioctl_access_superio_mmio handle 0x{:X} read offset 0x{:X} => empty response",
                    handle, offset
                )),
                Err(e) => out.push(format!(
                    "ioctl_access_superio_mmio handle 0x{:X} offset 0x{:X} failed: {}",
                    handle, offset, e
                )),
            }
        }
    }

    // 4) Try alternate calling convention where the map handle is passed as last parameter (some implementations use this)
    if !map_handles.is_empty() {
        for &map_h in &map_handles {
            out.push(format!(
                "-- Testing access with map handle as last param 0x{:X}",
                map_h
            ));
            for &offset in &offsets {
                let params = [0u64, offset, 1u64, 0u64, map_h];
                match pm.call_function("IsaBridgeEC", "ioctl_access_superio_mmio", &params) {
                    Ok(v) if !v.is_empty() => {
                        let hexs: Vec<String> = v.iter().map(|x| format!("0x{:X}", x)).collect();
                        out.push(format!(
                            "ioctl_access_superio_mmio (map-last) read offset 0x{:X} => {:?}",
                            offset, hexs
                        ));
                    }
                    Ok(_) => out.push(format!(
                        "ioctl_access_superio_mmio (map-last) read offset 0x{:X} => empty response",
                        offset
                    )),
                    Err(e) => out.push(format!(
                        "ioctl_access_superio_mmio (map-last) offset 0x{:X} failed: {}",
                        offset, e
                    )),
                }
            }

            // Additional attempts: try reversed-byte handle and varying sizes
            let rev = map_h.swap_bytes();
            out.push(format!(
                "-- Testing access with reversed-byte handle 0x{:X}",
                rev
            ));
            for &size in &[1u64, 2u64, 4u64] {
                for &offset in &offsets {
                    let params = [rev, offset, size, 0u64, 0u64];
                    match pm.call_function("IsaBridgeEC", "ioctl_access_superio_mmio", &params) {
                        Ok(v) if !v.is_empty() => {
                            let hexs: Vec<String> = v.iter().map(|x| format!("0x{:X}", x)).collect();
                            out.push(format!("ioctl_access_superio_mmio (rev-handle) size {} read offset 0x{:X} => {:?}", size, offset, hexs));
                        }
                        Ok(_) => out.push(format!("ioctl_access_superio_mmio (rev-handle) size {} read offset 0x{:X} => empty response", size, offset)),
                        Err(e) => out.push(format!("ioctl_access_superio_mmio (rev-handle) size {} offset 0x{:X} failed: {}", size, offset, e)),
                    }
                }
            }

            // Try reversed handle as last param too
            out.push(format!(
                "-- Testing access with reversed handle as last param 0x{:X}",
                rev
            ));
            for &size in &[1u64, 2u64, 4u64] {
                for &offset in &offsets {
                    let params = [0u64, offset, size, 0u64, rev];
                    match pm.call_function("IsaBridgeEC", "ioctl_access_superio_mmio", &params) {
                        Ok(v) if !v.is_empty() => {
                            let hexs: Vec<String> = v.iter().map(|x| format!("0x{:X}", x)).collect();
                            out.push(format!("ioctl_access_superio_mmio (rev-last) size {} read offset 0x{:X} => {:?}", size, offset, hexs));
                        }
                        Ok(_) => out.push(format!("ioctl_access_superio_mmio (rev-last) size {} read offset 0x{:X} => empty response", size, offset)),
                        Err(e) => out.push(format!("ioctl_access_superio_mmio (rev-last) size {} offset 0x{:X} failed: {}", size, offset, e)),
                    }
                }
            }
        }
    }

    // 5) Additional permutation tests: try many possible parameter orders with returned handles
    if !map_handles.is_empty() {
        out.push("-- Running permutation tests for ioctl_access_superio_mmio".to_string());
        for &map_h in &map_handles {
            let rev = map_h.swap_bytes();
            // candidate param orders to try (read-only: is_write=0)
            let permutations: Vec<Vec<u64>> = vec![
                vec![map_h, 0, 1, 0, 0], // handle, offset, size, is_write, value
                vec![map_h, 0, 0, 1, 0], // handle, offset, is_write, size, value
                vec![0, 0, 1, 0, map_h], // offset, size, is_write, value, handle
                vec![0, 1, 0, map_h, 0], // offset, size, is_write, handle, value
                vec![0, map_h, 1, 0, 0], // offset, handle, size, is_write, value
                vec![1, map_h, 0, 0, 0], // size, handle, offset, is_write, value
                vec![map_h, 1, 0, 0, 0], // handle, size, is_write, is_writeFlag, dummy
                vec![rev, 0, 1, 0, 0],   // reversed handle variants
                vec![0, rev, 1, 0, 0],
                vec![0, 0, 1, 0, rev],
            ];

            for params in permutations {
                out.push(format!(
                    "-- Perm test params: {:?}",
                    params
                        .iter()
                        .map(|p| format!("0x{:X}", p))
                        .collect::<Vec<_>>()
                ));
                match pm.call_function("IsaBridgeEC", "ioctl_access_superio_mmio", &params) {
                    Ok(v) if !v.is_empty() => {
                        let hexs: Vec<String> = v.iter().map(|x| format!("0x{:X}", x)).collect();
                        out.push(format!(
                            "ioctl_access_superio_mmio permutation read => {:?}",
                            hexs
                        ));
                    }
                    Ok(_) => out.push(
                        "ioctl_access_superio_mmio permutation read => empty response".to_string(),
                    ),
                    Err(e) => out.push(format!(
                        "ioctl_access_superio_mmio permutation failed: {}",
                        e
                    )),
                }
            }
        }
    }

    // 6) Unmap (best-effort) - try both no-arg and passing handle
    if map_handles.is_empty() {
        let _ = pm.call_function("IsaBridgeEC", "ioctl_unmap_superio_mmio", &[]);
        out.push("ioctl_unmap_superio_mmio (no-arg) attempted".to_string());
    } else {
        for &h in &map_handles {
            let _ = pm.call_function("IsaBridgeEC", "ioctl_unmap_superio_mmio", &[h]);
            out.push(format!(
                "ioctl_unmap_superio_mmio attempted for handle 0x{:X}",
                h
            ));
        }
    }

    out.push("EC diagnostic complete".to_string());
    out
}

/// Attempt to find MMIO regions for SuperIO using `IsaBridgeEC`.
/// Returns the raw vector from the driver (usually 6 values: base,size,chip ... etc) on success.
pub fn find_superio_mmio(pm: &mut PawnModuleManager) -> Option<Vec<u64>> {
    match pm.call_function("IsaBridgeEC", "ioctl_find_superio_mmio", &[]) {
        Ok(vec) => {
            if vec.is_empty() {
                None
            } else {
                Some(vec)
            }
        }
        Err(_) => None,
    }
}
