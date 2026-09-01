use std::io::Read;

use super::*;

#[test]
fn kitty_direct_frame_sequence_transmits_rgb_at_requested_area() {
    let frame = [0, 0, 0, 255, 255, 255];
    let area = ImageArea {
        x: 1,
        y: 2,
        cols: 3,
        rows: 4,
    };
    let mut out = Vec::new();
    let mut scratch = Vec::new();

    write_kitty_direct_image(
        &mut out,
        KittyFramePlacement {
            image_id: 7,
            placement_id: 9,
            z_index: 11,
            previous_image_id: None,
            width: 2,
            height: 1,
            area,
        },
        &frame,
        &mut scratch,
    )
    .expect("kitty frame should encode");

    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("\x1b[3;2H"));
    assert!(text.contains("a=T,q=2,f=24,s=2,v=1,i=7,p=9,c=3,r=4,C=1,z=11,m=0;"));
    assert!(text.contains("AAAA////"));
}

#[test]
fn kitty_direct_frame_sequence_chunks_large_frames() {
    let frame = vec![0xFF; KITTY_RAW_CHUNK_BYTES + 3];
    let placement = KittyFramePlacement {
        image_id: 7,
        placement_id: 9,
        z_index: 11,
        previous_image_id: None,
        width: 1,
        height: 1,
        area: ImageArea {
            x: 0,
            y: 0,
            cols: 1,
            rows: 1,
        },
    };
    let mut out = Vec::new();
    let mut scratch = Vec::new();

    write_kitty_direct_image(&mut out, placement, &frame, &mut scratch)
        .expect("kitty frame should encode");

    let text = String::from_utf8_lossy(&out);
    assert_eq!(text.matches("a=T,").count(), 1);
    assert_eq!(text.matches("\x1b_Gm=0;").count(), 1);
    assert!(text.contains("z=11,m=1;"));
}

#[test]
fn kitty_shared_memory_sequence_transmits_only_the_object_name() {
    let frame = [0, 1, 2, 3, 4, 5];
    let shared_frame =
        SharedMemoryFrame::create(&frame).expect("shared memory frame should be created");
    let placement = KittyFramePlacement {
        image_id: 7,
        placement_id: 9,
        z_index: 11,
        previous_image_id: Some(6),
        width: 2,
        height: 1,
        area: ImageArea {
            x: 1,
            y: 2,
            cols: 3,
            rows: 4,
        },
    };
    let mut out = Vec::new();
    let mut scratch = Vec::new();

    write_kitty_shared_memory_image(&mut out, placement, shared_frame.name(), &mut scratch)
        .expect("shared memory command should encode");

    let text = String::from_utf8_lossy(&out);
    let encoded_name = BASE64.encode(shared_frame.name().to_bytes());
    assert!(text.contains("a=T,q=2,f=24,t=s,s=2,v=1,i=7,p=9,c=3,r=4,C=1,z=11;"));
    assert!(text.contains(&encoded_name));
    assert!(!out.windows(frame.len()).any(|window| window == frame));
    assert!(text.contains("a=d,d=I,q=2,i=6"));
}

#[test]
fn shared_memory_frame_contains_the_frame_and_cleans_up_while_owned() {
    let frame = [0, 1, 2, 3, 4, 5];
    let shared_frame =
        SharedMemoryFrame::create(&frame).expect("shared memory frame should be created");
    let name = shared_frame.name().to_owned();
    let fd = unsafe { libc::shm_open(name.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC, 0) };
    assert!(fd >= 0, "shared memory frame should be reopenable");
    let mut file = unsafe { File::from_raw_fd(fd) };
    let mut stored_frame = Vec::new();
    file.read_to_end(&mut stored_frame)
        .expect("shared frame should be readable");
    assert_eq!(stored_frame, frame);

    drop(shared_frame);

    let fd = unsafe { libc::shm_open(name.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC, 0) };
    assert_eq!(fd, -1, "owned shared memory frame should be unlinked");
}

#[test]
fn shared_memory_is_selected_for_local_kitty_or_tmux_sessions() {
    assert!(is_shared_memory_preferred(false, true, false));
    assert!(is_shared_memory_preferred(true, false, false));
    assert!(is_shared_memory_preferred(true, true, false));
    assert!(!is_shared_memory_preferred(false, false, false));
    assert!(!is_shared_memory_preferred(false, true, true));
    assert!(!is_shared_memory_preferred(true, false, true));
}
