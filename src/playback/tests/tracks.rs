use super::*;

#[test]
fn catalog_keeps_disabled_and_default_audio_distinct() {
    let track = AudioTrack::default_track();
    let mut catalog = AudioCatalog {
        labels: Arc::from([Arc::<str>::from(track.label())]),
        tracks: vec![track],
        selected: Some(0),
    };

    assert_eq!(catalog.choice(), AudioChoice::Default);
    assert_eq!(catalog.labels()[0].as_ref(), "Default");

    catalog.select(None);

    assert_eq!(catalog.choice(), AudioChoice::Off);
    assert_eq!(catalog.selected(), None);
}
