use bevy::{
	asset::{Asset, AssetPath, AssetServer, Handle},
	prelude::{Image, Res},
};
use bevy_tileset_tiles::prelude::*;

pub trait TextureLoader {
	fn load_texture<'a, T: Asset, P: Into<AssetPath<'a>>>(&mut self, path: P) -> Handle<Image>;
}

impl TextureLoader for AssetServer {
	fn load_texture<'a, T: Asset, P: Into<AssetPath<'a>>>(&mut self, path: P) -> Handle<Image> {
		self.load(path)
	}
}

impl<'w> TextureLoader for Res<'w, AssetServer> {
	fn load_texture<'a, T: Asset, P: Into<AssetPath<'a>>>(&mut self, path: P) -> Handle<Image> {
		self.load(path)
	}
}

/// Load the intermediate tile handles from a set of tile definitions
///
/// This can then be used to generate a new [`RawTileset`](crate::tileset::RawTileset)
///
/// # Arguments
///
/// * `tiles`: The collection of tile definitions (in their intended order)
/// * `asset_loader`: The `AssetServer` or other loader for loading the textures
///
/// returns: Vec<TileHandle, Global>
///
/// # Examples
///
/// ```
/// # use bevy_tileset_core::prelude::*;
/// # use bevy_tileset_tiles::prelude::*;
/// # use bevy::prelude::*;
///
/// fn create_handles(tiles: Vec<TileDef>, asset_server: &AssetServer) -> Vec<TileHandle> {
/// 	load_tile_handles(tiles, asset_server)
/// }
///
/// fn is_ready(tiles: &Vec<TileHandle>, asset_server: &AssetServer) -> bool {
/// 	tiles.iter().all(|tile| tile.is_loaded(asset_server) )
/// }
/// ```
pub fn load_tile_handles<TTiles: IntoIterator<Item = TileDef>, TLoader: TextureLoader>(
	tiles: TTiles,
	asset_loader: &mut TLoader,
) -> Vec<TileHandle> {
	tiles
		.into_iter()
		.map(|tile_def| TileHandle {
			name: tile_def.name.clone(),
			tile: match tile_def.tile {
				TileDefType::Standard(path) => {
					TileHandleType::Standard(asset_loader.load_texture::<Image, String>(path))
				},
				TileDefType::Animated(anim) => {
					TileHandleType::Animated(load_animated(anim.clone(), asset_loader))
				},
				#[cfg(feature = "variants")]
				TileDefType::Variant(variants) => TileHandleType::Variant(
					variants
						.iter()
						.map(|variant| load_variant(variant, asset_loader))
						.collect(),
				),
				#[cfg(feature = "auto-tile")]
				TileDefType::Auto(autos) => TileHandleType::Auto(
					autos
						.iter()
						.map(|auto| load_auto(auto, asset_loader))
						.collect(),
				),
			},
		})
		.collect::<Vec<_>>()
}

fn load_animated<TLoader: TextureLoader>(
	def: AnimatedTileDef,
	asset_loader: &mut TLoader,
) -> AnimatedTileHandle {
	AnimatedTileHandle {
		speed: def.speed,
		frames: def
			.frames
			.iter()
			.map(|frame| asset_loader.load_texture::<Image, String>(frame.to_string()))
			.collect(),
	}
}

#[cfg(feature = "variants")]
fn load_variant<TLoader: TextureLoader>(
	def: &VariantTileDef,
	asset_loader: &mut TLoader,
) -> VariantTileHandle {
	VariantTileHandle {
		weight: def.weight,
		tile: match &def.tile {
			SimpleTileDefType::Standard(path) => SimpleTileHandle::Standard(
				asset_loader.load_texture::<Image, String>(path.to_string()),
			),
			SimpleTileDefType::Animated(anim) => {
				SimpleTileHandle::Animated(load_animated(anim.clone(), asset_loader))
			},
		},
	}
}

#[cfg(feature = "auto-tile")]
fn load_auto<TLoader: TextureLoader>(
	def: &AutoTileDef,
	asset_loader: &mut TLoader,
) -> AutoTileHandle {
	AutoTileHandle {
		rule: def.rule,
		variants: def
			.variants
			.iter()
			.map(|variant| load_variant(variant, asset_loader))
			.collect(),
	}
}
