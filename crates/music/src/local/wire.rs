use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::Accessor;
use lofty::probe::Probe;
use lofty::tag::Tag;

use crate::{
    Album, ArtistRef, LOCAL_ALBUM_PREFIX, LOCAL_ARTIST_PREFIX, LOCAL_TRACK_PREFIX, ReleaseType,
    Track,
};

const COVER_NAMES: &[&str] = &[
    "cover.jpg",
    "cover.jpeg",
    "cover.png",
    "cover.webp",
    "folder.jpg",
    "folder.jpeg",
    "folder.png",
    "folder.webp",
];

const ARTIST_NAMES: &[&str] = &[
    "artist.jpg",
    "artist.jpeg",
    "artist.png",
    "artist.webp",
    "folder.jpg",
    "folder.jpeg",
    "folder.png",
    "folder.webp",
    "cover.jpg",
    "cover.jpeg",
    "cover.png",
    "cover.webp",
];

const PLAYABLE_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "m4a", "mp4", "aac", "ogg", "oga", "wav", "opus",
];

fn is_playable(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            PLAYABLE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        })
}

pub fn track_id(path: &Path) -> String {
    format!("{LOCAL_TRACK_PREFIX}{}", path.display())
}

pub fn path_from_track_id(id: &str) -> Option<&Path> {
    id.strip_prefix(LOCAL_TRACK_PREFIX).map(Path::new)
}

pub fn album_id(dir: &Path) -> String {
    format!("{LOCAL_ALBUM_PREFIX}{}", dir.display())
}

pub fn path_from_album_id(id: &str) -> Option<&Path> {
    id.strip_prefix(LOCAL_ALBUM_PREFIX).map(Path::new)
}

pub fn artist_id(name: &str) -> String {
    format!("{LOCAL_ARTIST_PREFIX}{name}")
}

pub fn artist_name_from_id(id: &str) -> Option<&str> {
    id.strip_prefix(LOCAL_ARTIST_PREFIX)
}

fn artist_ref(name: &str) -> ArtistRef {
    ArtistRef {
        name: name.to_owned(),
        id: Some(artist_id(name)),
    }
}

fn clean(value: Option<std::borrow::Cow<'_, str>>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// When a file last changed, in whole seconds since the epoch. `None` when the
/// filesystem does not keep the time or it sits outside what an `i64` of seconds holds.
pub fn modified_at(path: &Path) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    match modified.duration_since(std::time::UNIX_EPOCH) {
        Ok(since) => i64::try_from(since.as_secs()).ok(),
        Err(before) => i64::try_from(before.duration().as_secs())
            .ok()
            .map(|seconds| -seconds),
    }
}

pub fn track_from_file(
    path: &Path,
    artist_hint: Option<&str>,
    album: Option<(&str, &Path)>,
    cache_dir: &Path,
) -> Option<Track> {
    let tagged = Probe::open(path).ok()?.read().ok();
    let tag = tagged
        .as_ref()
        .and_then(|file| file.primary_tag().or_else(|| file.first_tag()));

    let name = clean(tag.and_then(Accessor::title)).unwrap_or_else(|| file_stem(path));
    let artist = clean(tag.and_then(Accessor::artist))
        .or_else(|| artist_hint.map(str::to_owned))
        .unwrap_or_else(|| "Unknown Artist".to_owned());
    let album_name = clean(tag.and_then(Accessor::album))
        .or_else(|| album.map(|(name, _)| name.to_owned()))
        .unwrap_or_default();
    let album_dir = album.map(|(_, dir)| dir);
    let track_number = tag.and_then(Accessor::track).unwrap_or(0);
    let disc_number = tag.and_then(Accessor::disk).unwrap_or(0);
    let duration = tagged
        .as_ref()
        .map(|file| file.properties().duration())
        .unwrap_or_default();
    let cover = extract_cover(tag, path, album_dir, cache_dir);

    Some(Track {
        id: Some(track_id(path)),
        name,
        playable: is_playable(path),
        artists: artist.clone(),
        artist_refs: vec![artist_ref(&artist)],
        album: album_name,
        album_id: album_dir.map(album_id),
        cover,
        duration,
        added_at: modified_at(path),
        added_by: None,
        playcount: None,
        popularity: 0,
        explicit: false,
        track_number,
        disc_number,
        tags: Vec::new(),
        languages: Vec::new(),
        credits: Vec::new(),
    })
}

pub fn tag_year(path: &Path) -> Option<i32> {
    let tagged = Probe::open(path).ok()?.read().ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    tag.date()
        .map(|date| date.year as i32)
        .filter(|year| *year > 0)
}

pub fn album_from_tracks(
    name: &str,
    artist: &str,
    dir: &Path,
    tracks: &[Track],
    year: i32,
) -> Album {
    let cover = folder_cover(dir).or_else(|| tracks.iter().find_map(|track| track.cover.clone()));
    Album {
        id: album_id(dir),
        name: name.to_owned(),
        artists: artist.to_owned(),
        artist_refs: vec![artist_ref(artist)],
        cover: cover.clone(),
        cover_large: cover,
        release_type: ReleaseType::Album,
        year,
        track_count: tracks.len() as u32,
        release_date: String::new(),
        label: String::new(),
        copyrights: Vec::new(),
        added_at: tracks.iter().filter_map(|track| track.added_at).max(),
    }
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

pub fn folder_cover(dir: &Path) -> Option<String> {
    beside(dir, COVER_NAMES)
}

pub fn artist_cover(dir: &Path) -> Option<String> {
    beside(dir, ARTIST_NAMES)
}

fn beside(dir: &Path, names: &[&str]) -> Option<String> {
    names
        .iter()
        .map(|name| dir.join(name))
        .find(|candidate| candidate.is_file())
        .map(|candidate| format!("file://{}", candidate.display()))
}

fn extract_cover(
    tag: Option<&Tag>,
    path: &Path,
    album_dir: Option<&Path>,
    cache_dir: &Path,
) -> Option<String> {
    let picture = tag.and_then(|tag| {
        tag.pictures()
            .iter()
            .find(|picture| picture.pic_type() == PictureType::CoverFront)
            .or_else(|| tag.pictures().first())
    });
    if let Some(picture) = picture
        && let Some(cached) = cache_picture(picture, cache_dir)
    {
        return Some(cached);
    }

    path.parent()
        .and_then(folder_cover)
        .or_else(|| album_dir.and_then(folder_cover))
}

fn cache_picture(picture: &Picture, cache_dir: &Path) -> Option<String> {
    let extension = match picture.mime_type() {
        Some(MimeType::Png) => "png",
        Some(MimeType::Gif) => "gif",
        Some(MimeType::Bmp) => "bmp",
        Some(MimeType::Tiff) => "tiff",
        _ => "jpg",
    };

    let mut hasher = DefaultHasher::new();
    picture.data().hash(&mut hasher);
    let hash = hasher.finish();

    let dir = cache_dir.join("local-covers");
    std::fs::create_dir_all(&dir).ok()?;
    let dest = dir.join(format!("{hash:x}.{extension}"));
    if !dest.exists() {
        std::fs::write(&dest, picture.data()).ok()?;
    }
    Some(format!("file://{}", dest.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_track_is_dated_by_the_file_it_came_from() {
        let dir = scratch("sonora-wire-test-dated");
        let path = dir.join("song.mp3");
        std::fs::write(&path, []).unwrap();

        let stamped = modified_at(&path).expect("a file just written has a modified time");
        let track = track_from_file(&path, None, None, &dir).expect("a track");

        assert_eq!(track.added_at, Some(stamped));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_album_is_dated_by_its_newest_track() {
        let dir = scratch("sonora-wire-test-album-dated");
        let path = dir.join("song.mp3");
        std::fs::write(&path, []).unwrap();

        let mut older = track_from_file(&path, None, None, &dir).expect("a track");
        let mut newer = older.clone();
        older.added_at = Some(1_000);
        newer.added_at = Some(2_000);

        let album = album_from_tracks("Album", "Artist", &dir, &[older, newer], 2026);

        assert_eq!(album.added_at, Some(2_000));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_file_has_no_date() {
        let dir = scratch("sonora-wire-test-missing");
        assert_eq!(modified_at(&dir.join("gone.mp3")), None);
        std::fs::remove_dir_all(&dir).ok();
    }
}
