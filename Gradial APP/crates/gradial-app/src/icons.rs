use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    fs::{self, OpenOptions},
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::RwLock,
};

use eframe::egui::{ColorImage, Context, TextureHandle, TextureId, TextureOptions};
use gradial_core::{Configuration, IconFormat, IconReference};
use image::{DynamicImage, ImageDecoder, codecs::png::PngDecoder};
use thiserror::Error;
use uuid::Uuid;

pub(crate) const MAX_ICON_SOURCE_BYTES: u64 = 256 * 1024;
pub(crate) const MAX_ICON_DIMENSION: u32 = 512;
const MAX_ICON_DIMENSION_F32: f32 = 512.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum IconVisual {
    Texture { texture_id: TextureId, dimensions: [u32; 2] },
    Placeholder,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct IconStorageReconciliation {
    pub(crate) protected: usize,
    pub(crate) restored: usize,
    pub(crate) quarantined: usize,
    pub(crate) conflicts: usize,
    pub(crate) unknown: usize,
    pub(crate) errors: usize,
    pub(crate) quarantine_allowed: bool,
}

enum CachedIcon {
    Texture { texture: TextureHandle, dimensions: [u32; 2] },
    Placeholder,
}

impl std::fmt::Debug for CachedIcon {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Texture { dimensions, .. } => formatter
                .debug_struct("Texture")
                .field("dimensions", dimensions)
                .finish_non_exhaustive(),
            Self::Placeholder => formatter.write_str("Placeholder"),
        }
    }
}

#[derive(Debug)]
pub(crate) struct IconManager {
    directory: PathBuf,
    entries: RwLock<HashMap<IconReference, CachedIcon>>,
}

impl IconManager {
    pub(crate) fn new(directory: PathBuf) -> Self {
        Self { directory, entries: RwLock::new(HashMap::new()) }
    }

    pub(crate) fn preload_configuration(&self, context: &Context, configuration: &Configuration) {
        self.load_configuration(context, configuration, false);
    }

    pub(crate) fn refresh_configuration(&self, context: &Context, configuration: &Configuration) {
        self.load_configuration(context, configuration, true);
    }

    pub(crate) fn reconcile_storage(
        &self,
        protected: &HashSet<IconReference>,
        quarantine_allowed: bool,
        config_revision: u64,
    ) -> IconStorageReconciliation {
        let quarantine = self.directory.join("quarantine");
        let mut summary = IconStorageReconciliation {
            protected: protected.len(),
            quarantine_allowed,
            ..IconStorageReconciliation::default()
        };
        let quarantine_exists = match fs::symlink_metadata(&quarantine) {
            Ok(metadata) if metadata.file_type().is_dir() => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Ok(_) | Err(_) => {
                summary.errors = summary.errors.saturating_add(1);
                false
            }
        };
        if quarantine_exists {
            self.restore_protected(protected, &quarantine, &mut summary);
        }
        if quarantine_allowed && (quarantine_exists || summary.errors == 0) {
            self.quarantine_unreferenced(protected, &quarantine, &mut summary);
        }
        log::info!(
            target: "settings",
            "component=settings event_code=icon_storage_reconciled result_code={} \
             config_revision={} protected_count={} restored_count={} quarantined_count={} \
             conflict_count={} unknown_count={} error_count={} quarantine_allowed={}",
            if summary.errors == 0 { "ok" } else { "degraded" },
            config_revision,
            summary.protected,
            summary.restored,
            summary.quarantined,
            summary.conflicts,
            summary.unknown,
            summary.errors,
            summary.quarantine_allowed
        );
        summary
    }

    fn load_configuration(&self, context: &Context, configuration: &Configuration, refresh: bool) {
        let references = configuration_icon_references(configuration);
        let mut loaded = 0_usize;
        let mut placeholders = 0_usize;
        for reference in references.iter().copied() {
            if !refresh && self.has_texture(reference) {
                continue;
            }
            match self.load_managed(context, reference) {
                Ok(cached) => {
                    self.insert(reference, cached);
                    loaded = loaded.saturating_add(1);
                }
                Err(error) => {
                    self.insert(reference, CachedIcon::Placeholder);
                    placeholders = placeholders.saturating_add(1);
                    log::warn!(
                        target: "renderer",
                        "component=renderer event_code=icon_placeholder_selected \
                         result_code=fallback config_revision={} asset_id={} format={} \
                         error_code={}",
                        configuration.config_revision,
                        reference.asset_id,
                        reference.format.extension(),
                        error.code()
                    );
                }
            }
        }
        log::info!(
            target: "renderer",
            "component=renderer event_code=icon_cache_preloaded result_code=ok \
             config_revision={} referenced_count={} newly_loaded={} placeholder_count={} \
             cached_count={} refresh={}",
            configuration.config_revision,
            references.len(),
            loaded,
            placeholders,
            self.cached_count(),
            refresh
        );
    }

    fn restore_protected(
        &self,
        protected: &HashSet<IconReference>,
        quarantine: &Path,
        summary: &mut IconStorageReconciliation,
    ) {
        let mut references = protected.iter().copied().collect::<Vec<_>>();
        references.sort_unstable_by_key(|reference| reference.file_name());
        for reference in references {
            let managed = self.directory.join(reference.file_name());
            match fs::symlink_metadata(&managed) {
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => {
                    summary.errors = summary.errors.saturating_add(1);
                    continue;
                }
            }
            let source = quarantine.join(reference.file_name());
            let metadata = match fs::symlink_metadata(&source) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => {
                    summary.errors = summary.errors.saturating_add(1);
                    continue;
                }
            };
            if !metadata.file_type().is_file()
                || read_validated_icon_source(&source, reference.format).is_err()
            {
                summary.errors = summary.errors.saturating_add(1);
                continue;
            }
            match fs::rename(&source, &managed) {
                Ok(()) => summary.restored = summary.restored.saturating_add(1),
                Err(_) => summary.conflicts = summary.conflicts.saturating_add(1),
            }
        }
    }

    fn quarantine_unreferenced(
        &self,
        protected: &HashSet<IconReference>,
        quarantine: &Path,
        summary: &mut IconStorageReconciliation,
    ) {
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(_) => {
                summary.errors = summary.errors.saturating_add(1);
                return;
            }
        };
        let mut quarantine_ready = is_plain_directory(quarantine);
        for entry in entries {
            let Ok(entry) = entry else {
                summary.errors = summary.errors.saturating_add(1);
                continue;
            };
            let Ok(file_type) = entry.file_type() else {
                summary.errors = summary.errors.saturating_add(1);
                continue;
            };
            if !file_type.is_file() {
                summary.unknown = summary.unknown.saturating_add(1);
                continue;
            }
            let Some(reference) = managed_icon_reference(&entry.file_name()) else {
                summary.unknown = summary.unknown.saturating_add(1);
                continue;
            };
            if protected.contains(&reference) {
                continue;
            }
            if !quarantine_ready {
                match fs::create_dir(quarantine) {
                    Ok(()) => quarantine_ready = true,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::AlreadyExists
                            && is_plain_directory(quarantine) =>
                    {
                        quarantine_ready = true;
                    }
                    Err(_) => {
                        summary.errors = summary.errors.saturating_add(1);
                        return;
                    }
                }
            }
            let destination = quarantine.join(reference.file_name());
            match fs::symlink_metadata(&destination) {
                Ok(_) => {
                    summary.conflicts = summary.conflicts.saturating_add(1);
                    continue;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => {
                    summary.errors = summary.errors.saturating_add(1);
                    continue;
                }
            }
            match fs::rename(entry.path(), destination) {
                Ok(()) => {
                    self.entries
                        .write()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .remove(&reference);
                    summary.quarantined = summary.quarantined.saturating_add(1);
                }
                Err(_) => summary.errors = summary.errors.saturating_add(1),
            }
        }
    }

    pub(crate) fn import(
        &self,
        context: &Context,
        source: &Path,
        config_revision: u64,
    ) -> Result<IconReference, IconImportError> {
        if !source.is_absolute() {
            return Err(IconImportError::InvalidPath);
        }
        let format = icon_format_from_path(source)?;
        let bytes = read_bounded_source(source)?;
        let decoded = decode_icon(format, &bytes)?;
        let reference = IconReference { asset_id: Uuid::new_v4(), format };
        self.write_managed(reference, &bytes)?;
        let dimensions = decoded.dimensions;
        self.insert(reference, texture(context, reference, decoded));
        log::info!(
            target: "settings",
            "component=settings event_code=icon_imported result_code=ok \
             config_revision={config_revision} asset_id={} format={} width={} height={} \
             source_bytes={}",
            reference.asset_id,
            reference.format.extension(),
            dimensions[0],
            dimensions[1],
            bytes.len()
        );
        Ok(reference)
    }

    pub(crate) fn visual(&self, reference: IconReference) -> IconVisual {
        let entries = self.entries.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        match entries.get(&reference) {
            Some(CachedIcon::Texture { texture, dimensions }) => {
                IconVisual::Texture { texture_id: texture.id(), dimensions: *dimensions }
            }
            Some(CachedIcon::Placeholder) | None => IconVisual::Placeholder,
        }
    }

    pub(crate) fn cached_count(&self) -> usize {
        self.entries.read().unwrap_or_else(std::sync::PoisonError::into_inner).len()
    }

    fn has_texture(&self, reference: IconReference) -> bool {
        matches!(
            self.entries.read().unwrap_or_else(std::sync::PoisonError::into_inner).get(&reference),
            Some(CachedIcon::Texture { .. })
        )
    }

    fn insert(&self, reference: IconReference, cached: CachedIcon) {
        self.entries
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(reference, cached);
    }

    fn load_managed(
        &self,
        context: &Context,
        reference: IconReference,
    ) -> Result<CachedIcon, IconImportError> {
        let path = self.directory.join(reference.file_name());
        let bytes = read_bounded_source(&path)?;
        let decoded = decode_icon(reference.format, &bytes)?;
        Ok(texture(context, reference, decoded))
    }

    fn write_managed(&self, reference: IconReference, bytes: &[u8]) -> Result<(), IconImportError> {
        let destination = self.directory.join(reference.file_name());
        let temporary = self.directory.join(format!(".{}.tmp", reference.asset_id.as_hyphenated()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)
                .map_err(|_| IconImportError::Write)?;
            file.write_all(bytes).map_err(|_| IconImportError::Write)?;
            file.sync_all().map_err(|_| IconImportError::Write)?;
            drop(file);
            fs::rename(&temporary, &destination).map_err(|_| IconImportError::Write)
        })();
        if result.is_err() {
            let _removed = fs::remove_file(&temporary);
        }
        result
    }
}

pub(crate) fn configuration_icon_references(configuration: &Configuration) -> Vec<IconReference> {
    let mut references = configuration
        .global_profile
        .menu
        .slices
        .iter()
        .chain(
            configuration
                .application_profiles
                .iter()
                .flat_map(|application| &application.profile.menu.slices),
        )
        .filter_map(|slice| slice.icon)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    references.sort_unstable_by_key(|reference| reference.file_name());
    references
}

fn managed_icon_reference(file_name: &OsStr) -> Option<IconReference> {
    let name = file_name.to_str()?;
    let (asset_id, extension) = name.rsplit_once('.')?;
    let format = match extension {
        "png" => IconFormat::Png,
        "svg" => IconFormat::Svg,
        _ => return None,
    };
    let reference = IconReference { asset_id: Uuid::parse_str(asset_id).ok()?, format };
    (!reference.asset_id.is_nil() && name == reference.file_name()).then_some(reference)
}

fn is_plain_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_dir())
}

fn icon_format_from_path(path: &Path) -> Result<IconFormat, IconImportError> {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return Err(IconImportError::UnsupportedType);
    };
    if extension.eq_ignore_ascii_case("png") {
        Ok(IconFormat::Png)
    } else if extension.eq_ignore_ascii_case("svg") {
        Ok(IconFormat::Svg)
    } else {
        Err(IconImportError::UnsupportedType)
    }
}

fn read_bounded_source(path: &Path) -> Result<Vec<u8>, IconImportError> {
    let metadata = fs::metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            IconImportError::Missing
        } else {
            IconImportError::Read
        }
    })?;
    if !metadata.is_file() {
        return Err(IconImportError::InvalidPath);
    }
    if metadata.len() > MAX_ICON_SOURCE_BYTES {
        return Err(IconImportError::SourceTooLarge);
    }
    let bytes = fs::read(path).map_err(|_| IconImportError::Read)?;
    if bytes.is_empty() {
        return Err(IconImportError::Empty);
    }
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_ICON_SOURCE_BYTES {
        return Err(IconImportError::SourceTooLarge);
    }
    Ok(bytes)
}

pub(crate) fn read_validated_icon_source(
    path: &Path,
    format: IconFormat,
) -> Result<Vec<u8>, IconImportError> {
    let bytes = read_bounded_source(path)?;
    let _decoded = decode_icon(format, &bytes)?;
    Ok(bytes)
}

#[derive(Debug)]
struct DecodedIcon {
    image: ColorImage,
    dimensions: [u32; 2],
}

fn decode_icon(format: IconFormat, bytes: &[u8]) -> Result<DecodedIcon, IconImportError> {
    match format {
        IconFormat::Png => decode_png(bytes),
        IconFormat::Svg => decode_svg(bytes),
    }
}

fn decode_png(bytes: &[u8]) -> Result<DecodedIcon, IconImportError> {
    let decoder = PngDecoder::new(Cursor::new(bytes)).map_err(|_| IconImportError::InvalidPng)?;
    let (width, height) = decoder.dimensions();
    validate_dimensions(width, height)?;
    let rgba =
        DynamicImage::from_decoder(decoder).map_err(|_| IconImportError::InvalidPng)?.into_rgba8();
    let image = ColorImage::from_rgba_unmultiplied(
        [
            usize::try_from(width).map_err(|_| IconImportError::Dimensions)?,
            usize::try_from(height).map_err(|_| IconImportError::Dimensions)?,
        ],
        rgba.as_raw(),
    );
    Ok(DecodedIcon { image, dimensions: [width, height] })
}

fn decode_svg(bytes: &[u8]) -> Result<DecodedIcon, IconImportError> {
    let source = std::str::from_utf8(bytes).map_err(|_| IconImportError::InvalidSvg)?;
    let normalized = source.to_ascii_lowercase();
    if ["<!doctype", "<!entity", "<script", "<foreignobject", "<image", "<text"]
        .iter()
        .any(|unsupported| normalized.contains(unsupported))
    {
        return Err(IconImportError::UnsupportedSvgContent);
    }
    let options = resvg::usvg::Options::default();
    let tree =
        resvg::usvg::Tree::from_data(bytes, &options).map_err(|_| IconImportError::InvalidSvg)?;
    let logical_size = tree.size();
    if logical_size.width() > MAX_ICON_DIMENSION_F32
        || logical_size.height() > MAX_ICON_DIMENSION_F32
    {
        return Err(IconImportError::Dimensions);
    }
    let size = logical_size.to_int_size();
    let width = size.width();
    let height = size.height();
    validate_dimensions(width, height)?;
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(width, height).ok_or(IconImportError::Dimensions)?;
    resvg::render(&tree, resvg::tiny_skia::Transform::identity(), &mut pixmap.as_mut());
    let image = ColorImage::from_rgba_premultiplied(
        [
            usize::try_from(width).map_err(|_| IconImportError::Dimensions)?,
            usize::try_from(height).map_err(|_| IconImportError::Dimensions)?,
        ],
        pixmap.data(),
    );
    Ok(DecodedIcon { image, dimensions: [width, height] })
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), IconImportError> {
    if width == 0 || height == 0 || width > MAX_ICON_DIMENSION || height > MAX_ICON_DIMENSION {
        return Err(IconImportError::Dimensions);
    }
    Ok(())
}

fn texture(context: &Context, reference: IconReference, decoded: DecodedIcon) -> CachedIcon {
    let texture = context.load_texture(
        format!("gradial-icon-{}", reference.asset_id.as_hyphenated()),
        decoded.image,
        TextureOptions::LINEAR,
    );
    CachedIcon::Texture { texture, dimensions: decoded.dimensions }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub(crate) enum IconImportError {
    #[error("the selected icon path is invalid")]
    InvalidPath,
    #[error("the selected icon does not exist")]
    Missing,
    #[error("the selected icon could not be read")]
    Read,
    #[error("only PNG and SVG icons are supported")]
    UnsupportedType,
    #[error("the icon source is empty")]
    Empty,
    #[error("the icon source is larger than 256 KiB")]
    SourceTooLarge,
    #[error("the PNG source is invalid")]
    InvalidPng,
    #[error("the SVG source is invalid")]
    InvalidSvg,
    #[error("SVG text, embedded images, scripts, and external document features are unsupported")]
    UnsupportedSvgContent,
    #[error("icon dimensions must be between 1 and 512 logical pixels")]
    Dimensions,
    #[error("the icon could not be copied into Gradial's managed storage")]
    Write,
}

impl IconImportError {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::InvalidPath => "invalid_path",
            Self::Missing => "missing",
            Self::Read => "read_failed",
            Self::UnsupportedType => "unsupported_type",
            Self::Empty => "empty",
            Self::SourceTooLarge => "source_too_large",
            Self::InvalidPng => "invalid_png",
            Self::InvalidSvg => "invalid_svg",
            Self::UnsupportedSvgContent => "unsupported_svg_content",
            Self::Dimensions => "invalid_dimensions",
            Self::Write => "write_failed",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, ffi::OsStr, fs, path::PathBuf};

    use eframe::egui::Context;
    use gradial_core::{Configuration, IconFormat, IconReference};
    use image::{ImageEncoder, codecs::png::PngEncoder};

    use super::{
        IconImportError, IconManager, IconVisual, MAX_ICON_DIMENSION, decode_svg,
        icon_format_from_path, managed_icon_reference,
    };

    fn minimal_png() -> Vec<u8> {
        let mut bytes = Vec::new();
        PngEncoder::new(&mut bytes)
            .write_image(&[255, 96, 64, 255], 1, 1, image::ExtendedColorType::Rgba8)
            .expect("PNG fixture");
        bytes
    }

    #[test]
    fn extension_matching_is_closed_and_case_insensitive() {
        assert_eq!(
            icon_format_from_path(&PathBuf::from("sample.PNG")).expect("png"),
            IconFormat::Png
        );
        assert_eq!(
            icon_format_from_path(&PathBuf::from("sample.svg")).expect("svg"),
            IconFormat::Svg
        );
        assert_eq!(
            icon_format_from_path(&PathBuf::from("sample.ico")),
            Err(IconImportError::UnsupportedType)
        );
    }

    #[test]
    fn bounded_svg_decodes_and_disallowed_content_is_rejected() {
        let valid = br##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="24"><path d="M0 0h32v24H0z" fill="#ff6644"/></svg>"##;
        let decoded = decode_svg(valid).expect("bounded vector");
        assert_eq!(decoded.dimensions, [32, 24]);
        assert_eq!(
            decode_svg(
                br#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="24"><image href="x.png"/></svg>"#
            )
            .expect_err("embedded image"),
            IconImportError::UnsupportedSvgContent
        );
        let oversized = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="1"/>"#,
            MAX_ICON_DIMENSION + 1
        );
        assert_eq!(
            decode_svg(oversized.as_bytes()).expect_err("oversized"),
            IconImportError::Dimensions
        );
    }

    #[test]
    fn import_copies_to_managed_storage_and_preloads_a_texture() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let source = temporary.path().join("source.png");
        let managed = temporary.path().join("icons");
        fs::create_dir(&managed).expect("managed directory");
        fs::write(&source, minimal_png()).expect("fixture");
        let context = Context::default();
        let cache = IconManager::new(managed.clone());

        let reference = cache.import(&context, &source, 7).expect("import");

        assert!(managed.join(reference.file_name()).is_file());
        assert!(matches!(cache.visual(reference), IconVisual::Texture { .. }));

        let mut configuration = Configuration::default();
        configuration.global_profile.menu.slices[0].icon = Some(reference);
        let fresh = IconManager::new(managed);
        fresh.preload_configuration(&context, &configuration);
        assert!(matches!(fresh.visual(reference), IconVisual::Texture { .. }));
    }

    #[test]
    fn missing_managed_asset_uses_placeholder() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let manager = IconManager::new(temporary.path().to_path_buf());
        let context = Context::default();
        let mut configuration = Configuration::default();
        let reference = gradial_core::IconReference {
            asset_id: uuid::Uuid::from_u128(7),
            format: IconFormat::Svg,
        };
        configuration.global_profile.menu.slices[0].icon = Some(reference);

        manager.preload_configuration(&context, &configuration);

        assert_eq!(manager.visual(reference), IconVisual::Placeholder);
    }

    #[test]
    fn preload_retries_a_placeholder_after_the_managed_asset_is_restored() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let manager = IconManager::new(temporary.path().to_path_buf());
        let context = Context::default();
        let mut configuration = Configuration::default();
        let reference = gradial_core::IconReference {
            asset_id: uuid::Uuid::from_u128(8),
            format: IconFormat::Png,
        };
        configuration.global_profile.menu.slices[0].icon = Some(reference);

        manager.preload_configuration(&context, &configuration);
        assert_eq!(manager.visual(reference), IconVisual::Placeholder);
        fs::write(temporary.path().join(reference.file_name()), minimal_png())
            .expect("restored managed icon");
        manager.preload_configuration(&context, &configuration);

        assert!(matches!(manager.visual(reference), IconVisual::Texture { .. }));
    }

    #[test]
    fn managed_filename_recognition_is_canonical_and_closed() {
        let reference =
            IconReference { asset_id: uuid::Uuid::from_u128(9), format: IconFormat::Png };

        assert_eq!(managed_icon_reference(OsStr::new(&reference.file_name())), Some(reference));
        assert_eq!(managed_icon_reference(OsStr::new(&reference.file_name().to_uppercase())), None);
        assert_eq!(managed_icon_reference(OsStr::new("notes.png")), None);
        assert_eq!(
            managed_icon_reference(OsStr::new("00000000-0000-0000-0000-000000000000.svg")),
            None
        );
    }

    #[test]
    fn reconciliation_treats_missing_icon_storage_as_an_empty_clean_state() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let managed = temporary.path().join("icons");
        let icon_manager = IconManager::new(managed.clone());

        let summary = icon_manager.reconcile_storage(&HashSet::new(), true, 8);

        assert_eq!(
            summary,
            super::IconStorageReconciliation { quarantine_allowed: true, ..Default::default() }
        );
        assert!(!managed.exists());
    }

    #[test]
    fn reconciliation_quarantines_only_unprotected_canonical_files_without_overwriting() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let managed = temporary.path().join("icons");
        let quarantine = managed.join("quarantine");
        fs::create_dir_all(&quarantine).expect("storage directories");
        let icon_manager = IconManager::new(managed.clone());
        let protected =
            IconReference { asset_id: uuid::Uuid::from_u128(10), format: IconFormat::Png };
        let orphan = IconReference { asset_id: uuid::Uuid::from_u128(11), format: IconFormat::Png };
        let conflict =
            IconReference { asset_id: uuid::Uuid::from_u128(12), format: IconFormat::Png };
        let directory =
            IconReference { asset_id: uuid::Uuid::from_u128(13), format: IconFormat::Svg };
        let noncanonical =
            IconReference { asset_id: uuid::Uuid::from_u128(17), format: IconFormat::Png };
        fs::write(managed.join(protected.file_name()), minimal_png()).expect("protected icon");
        fs::write(managed.join(orphan.file_name()), minimal_png()).expect("orphan icon");
        fs::write(managed.join(conflict.file_name()), minimal_png()).expect("managed conflict");
        fs::write(quarantine.join(conflict.file_name()), minimal_png())
            .expect("quarantined conflict");
        fs::write(managed.join("notes.png"), b"unknown").expect("unknown file");
        fs::write(managed.join(noncanonical.file_name().to_uppercase()), b"unknown")
            .expect("noncanonical file");
        fs::create_dir(managed.join(directory.file_name())).expect("canonical-looking directory");

        let summary = icon_manager.reconcile_storage(&HashSet::from([protected]), true, 9);

        assert_eq!(summary.quarantined, 1);
        assert_eq!(summary.conflicts, 1);
        assert!(managed.join(protected.file_name()).is_file());
        assert!(!managed.join(orphan.file_name()).exists());
        assert!(quarantine.join(orphan.file_name()).is_file());
        assert!(managed.join(conflict.file_name()).is_file());
        assert!(quarantine.join(conflict.file_name()).is_file());
        assert!(managed.join("notes.png").is_file());
        assert!(managed.join(noncanonical.file_name().to_uppercase()).is_file());
        assert!(managed.join(directory.file_name()).is_dir());
    }

    #[test]
    fn reconciliation_restores_valid_live_assets_even_when_quarantine_is_disabled() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let managed = temporary.path().join("icons");
        let quarantine = managed.join("quarantine");
        fs::create_dir_all(&quarantine).expect("storage directories");
        let icon_manager = IconManager::new(managed.clone());
        let live = IconReference { asset_id: uuid::Uuid::from_u128(14), format: IconFormat::Png };
        let damaged =
            IconReference { asset_id: uuid::Uuid::from_u128(15), format: IconFormat::Png };
        let orphan = IconReference { asset_id: uuid::Uuid::from_u128(16), format: IconFormat::Png };
        fs::write(quarantine.join(live.file_name()), minimal_png()).expect("live quarantine");
        fs::write(quarantine.join(damaged.file_name()), b"damaged").expect("damaged quarantine");
        fs::write(managed.join(orphan.file_name()), minimal_png()).expect("managed orphan");

        let summary = icon_manager.reconcile_storage(&HashSet::from([live, damaged]), false, 10);

        assert_eq!(summary.restored, 1);
        assert_eq!(summary.quarantined, 0);
        assert_eq!(summary.errors, 1);
        assert!(!summary.quarantine_allowed);
        assert!(managed.join(live.file_name()).is_file());
        assert!(!quarantine.join(live.file_name()).exists());
        assert!(quarantine.join(damaged.file_name()).is_file());
        assert!(!managed.join(damaged.file_name()).exists());
        assert!(managed.join(orphan.file_name()).is_file());
    }
}
