use std::{
	collections::{BTreeMap, HashMap},
	path::{Path, PathBuf},
	sync::{Arc, RwLock},
};

use bevy::{
	asset::{
		io::Reader,
		Asset,
		AssetLoader,
		AssetPath,
		AsyncReadExt,
		BoxedFuture,
		Handle,
		LoadContext,
	},
	prelude::{AssetId, FromWorld, World},
	render::{
		renderer::RenderDevice,
		texture::{CompressedImageFormats, Image, ImageSampler, ImageType},
	},
	utils::Uuid,
};
use bevy_tile_atlas::TextureStore;
use bevy_tileset_tiles::prelude::{TileDef, TileHandle};
use serde::{Deserialize, Serialize};

use crate::{
	prelude::{TileGroupId, Tileset, TilesetBuilder, TilesetError, TilesetId},
	tileset::load::{load_tile_handles, TextureLoader},
};

pub struct TilesetAssetLoader {
	supported_compressed_formats: CompressedImageFormats,
}

#[derive(Default, Deserialize, Serialize)]
pub struct TilesetDef {
	/// The optional name of the tileset (defaults to a random UUID string)
	pub name: Option<String>,
	/// The ID of the tileset
	pub id: TilesetId,
	/// The tiles in this tileset as a mapping of their group ID to the relative path to
	/// their definition file
	pub tiles: BTreeMap<TileGroupId, String>,
}

/// A struct that mimics a Bevy `AssetServer`
///
/// Instead of loading an image right away, it tracks the paths to the images to be loaded
/// later (so we don't need to await on _every_ image).
struct TilesetTextureLoader<'x, 'y> {
	supported_compressed_formats: CompressedImageFormats,
	load_context: &'x mut LoadContext<'y>,
	/// The images that need to be loaded
	bytes: Arc<RwLock<HashMap<AssetId<Image>, PathBuf>>>,
}

/// A struct that mimics a Bevy `Assets<Texture>` resource by allowing get/add operations
struct TilesetTextureStore<'x, 'y> {
	load_context: &'x mut LoadContext<'y>,
	images: HashMap<Handle<Image>, Image>,
}

impl<'x, 'y> TextureLoader for TilesetTextureLoader<'x, 'y> {
	fn load_texture<'a, T: Asset, P: Into<AssetPath<'a>>>(&mut self, path: P) -> Handle<Image> {
		let asset_path: AssetPath = path.into();
		let handle: Handle<Image> = self
			.load_context
			// FIXME unwrap
			.get_label_handle(asset_path.clone().to_string());
		let asset_path = asset_path.path();
		let path = asset_path.to_path_buf();

		if let Ok(mut images) = self.bytes.try_write() {
			images.insert(handle.id(), path);
		}
		handle
	}
}

/*
impl<'x, 'y> TilesetTextureLoader<'x, 'y> {
	/// Load the images and collect them into a HashMap
	fn collect_images(
		mut self,
	) -> BoxedFuture<'x, Result<HashMap<AssetId<Image>, Image>, TilesetError>> {
		let images = self.bytes.read().unwrap().clone();
		Box::pin(async move {
			let image_map = futures::future::join_all(images.into_iter().map(|(id, path)| {
				load_image(
					&mut self.load_context,
					id,
					path,
					self.supported_compressed_formats,
				)
			}))
			.await
			.into_iter()
			.filter_map(|x| x.ok())
			.collect();

			Ok(image_map)
		})
	}
}
	*/

impl<'x, 'y> TextureStore for TilesetTextureStore<'x, 'y> {
	fn add(&mut self, asset: Image) -> Handle<Image> {
		//! This should only really be called once: When creating the tile texture atlas
		//! since we'll need to track that asset as well.
		let prefix = self
			.load_context
			.path()
			.to_str()
			.unwrap_or("UNKNOWN_TILESET");
		let label = format!("Tileset__[{:?}]__{:?}", prefix, Uuid::new_v4().to_string());
		self.load_context.add_labeled_asset(label, asset)
	}

	fn get<H: Into<Handle<Image>>>(&self, handle: H) -> Option<&Image> {
		self.images.get(&handle.into())
	}
}

impl FromWorld for TilesetAssetLoader {
	fn from_world(world: &mut World) -> Self {
		let supported_compressed_formats = match world.get_resource::<RenderDevice>() {
			Some(render_device) => CompressedImageFormats::from_features(render_device.features()),

			None => CompressedImageFormats::all(),
		};
		Self {
			supported_compressed_formats,
		}
	}
}

impl AssetLoader for TilesetAssetLoader {
	type Asset = Tileset;
	type Settings = ();
	type Error = TilesetError;

	fn load<'a>(
		&'a self,
		reader: &'a mut Reader,
		_settings: &'a Self::Settings,
		load_context: &'a mut LoadContext,
	) -> BoxedFuture<'a, Result<Self::Asset, Self::Error>> {
		Box::pin(async move {
			let mut bytes = Vec::new();
			reader.read_to_end(&mut bytes).await?;

			let definition = ron::de::from_bytes::<TilesetDef>(&bytes)?;

			// === Load Handles === //
			let mut loader = TilesetTextureLoader {
				supported_compressed_formats: self.supported_compressed_formats,
				bytes: Arc::new(RwLock::new(HashMap::new())),
				load_context,
			};

			// FIXME
			let mut tile_defs: Vec<TileDef> = vec![];
			for (.., tile_path) in definition.tiles.iter() {
				let path = tile_path;
				let path = if let Some(parent) = loader.load_context.path().parent() {
					parent.join(path)
				} else {
					Path::new(&path).to_path_buf()
				};
				let bytes = loader
					.load_context
					.read_asset_bytes(path)
					.await
					.map_err(|err| TilesetError::ReadAssetBytesError(err))?;
				let def = ron::de::from_bytes::<TileDef>(&bytes)
					.map_err(|err| TilesetError::InvalidDefinition(err))?;
				tile_defs.push(def);
			}
			let handles = load_tile_handles(tile_defs, &mut loader);
			let tile_handles: Vec<(TileGroupId, TileHandle)> = definition
				.tiles
				.iter()
				.map(|(id, ..)| *id)
				.zip(handles.into_iter().map(|handle| handle))
				.collect();

			// === Build Tiles === //
			//let images = loader.collect_images().await?;
			let images = loader.bytes.read().unwrap().clone();
			let mut image_map = vec![];
			for (id, path) in images.into_iter() {
				let image =
					load_image(load_context, id, path, self.supported_compressed_formats).await;
				image_map.push(image);
			}
			let images = image_map
				.into_iter()
				.filter_map(|x| x.ok())
				// TODO not sure about the Weak Handle here
				.map(|(asset_id, image)| (Handle::Weak(asset_id), image))
				.collect();
			let mut store = TilesetTextureStore {
				load_context,
				images,
			};

			let mut builder = TilesetBuilder::default();
			for (group_id, tile_handle) in tile_handles {
				builder.add_tile(tile_handle, group_id, &store)?;
			}

			// === Create Raw Tileset === //
			let name = definition
				.name
				.unwrap_or_else(|| Uuid::new_v4().hyphenated().to_string());
			let raw_tileset = builder.build(name, definition.id, &mut store)?;

			// === Finalize Tileset === //
			let texture = raw_tileset.atlas().texture.clone();
			let atlas = load_context.add_labeled_asset("atlas".to_owned(), raw_tileset.atlas);
			let tileset = Tileset {
				id: raw_tileset.id,
				name: raw_tileset.name,
				tiles: raw_tileset.tiles,
				size: raw_tileset.size,
				tile_size: raw_tileset.tile_size,
				tile_ids: raw_tileset.tile_ids,
				tile_names: raw_tileset.tile_names,
				tile_handles: raw_tileset.tile_handles,
				tile_indices: raw_tileset.tile_indices,
				atlas,
				texture,
			};

			Ok(tileset)
		})
	}

	fn extensions(&self) -> &[&str] { &["ron"] }
}

/*
/// Get a `Vec` of ([`TileGroupId`], [`TileHandle`]) tuples
async fn get_tile_handles<'x, 'y>(
	loader: &'x mut TilesetTextureLoader<'x, 'y>,
	tile_paths: &BTreeMap<TileGroupId, String>,
) -> Result<Vec<(TileGroupId, TileHandle)>, TilesetError> {
	IoTaskPool::get()
		.scope(|scope| {
			let bytes = tile_paths.iter().map(|(.., tile_path)| {
				let path = tile_path;
				let path = if let Some(parent) = loader.load_context.path().parent() {
					parent.join(path)
				} else {
					Path::new(path).to_path_buf()
				};
				loader.load_context.read_asset_bytes(path)
			});

			for bytes in bytes {
				scope.spawn(async move {
					let bytes = bytes
						.await
						.map_err(|err| TilesetError::ReadAssetBytesError(err))?;
					let def = ron::de::from_bytes::<TileDef>(&bytes)
						.map_err(|err| TilesetError::InvalidDefinition(err))?;
					Ok(def)
				});
			}
		})
		.into_iter()
		.filter_map(|tile_def: Result<TileDef, TilesetError>| tile_def.ok())
		.collect::<Vec<_>>();
	/*
	let tile_defs = futures::future::join_all(
		tile_paths
			.iter()
			.map(|(.., tile_path)| load_tile(&mut loader.load_context, tile_path)),
	)
	.await
	.into_iter()
	.filter_map(|tile_def| tile_def.ok())
	.collect::<Vec<_>>();
	*/
	// FIXME
	//let tile_defs = vec![];

	//let handles = load_tile_handles(tile_defs, loader);
	let handles = vec![];

	Ok(tile_paths
		.iter()
		.map(|(id, ..)| *id)
		.zip(handles.into_iter().map(|handle| handle))
		.collect())
}
	*/

/// Load the tile definition at the given path and return its corresponding [TileDef]
///
/// The path is always relative to the tileset's configuration file path
/*
async fn load_tile<'x>(context: &mut LoadContext<'x>, path: &str) -> Result<TileDef, TilesetError> {
	let bytes = context
		.read_asset_bytes(path)
		.await
		.map_err(|err| TilesetError::ReadAssetBytesError(err))?;
	let def = ron::de::from_bytes::<TileDef>(&bytes)
		.map_err(|err| TilesetError::InvalidDefinition(err))?;
	Ok(def)
}
*/

/// Load an image at the given path
async fn load_image(
	context: &mut LoadContext<'_>,
	id: AssetId<Image>,
	path: PathBuf,
	supported_compressed_formats: CompressedImageFormats,
) -> Result<(AssetId<Image>, Image), TilesetError> {
	let bytes = context
		.read_asset_bytes(path.clone())
		.await
		.map_err(|err| TilesetError::ReadAssetBytesError(err))?;
	let path = path.as_path();
	let ext = path.extension().unwrap().to_str().unwrap();
	let img = Image::from_buffer(
		&bytes,
		ImageType::Extension(ext),
		supported_compressed_formats,
		true,
		ImageSampler::default(),
	)
	.map_err(|err| TilesetError::ImageError(err))?;
	Ok((id, img))
}
