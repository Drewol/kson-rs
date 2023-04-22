use std::rc::Rc;

use three_d::{Context, Geometry, Material, Object, Program};

pub struct RuscMat {
    program: Rc<Program>,
    context: Context,
}

impl Object for RuscMat {
    fn render(&self, _camera: &three_d::Camera, _lights: &[&dyn three_d::Light]) {}

    fn material_type(&self) -> three_d::MaterialType {
        todo!()
    }
}

impl Geometry for RuscMat {
    fn render_with_material(
        &self,
        _material: &dyn Material,
        _camera: &three_d::Camera,
        _lights: &[&dyn three_d::Light],
    ) {
        todo!()
    }

    fn render_with_post_material(
        &self,
        _material: &dyn three_d::PostMaterial,
        _camera: &three_d::Camera,
        _lights: &[&dyn three_d::Light],
        _color_texture: Option<three_d::ColorTexture>,
        _depth_texture: Option<three_d::DepthTexture>,
    ) {
        todo!()
    }

    fn aabb(&self) -> three_d::AxisAlignedBoundingBox {
        todo!()
    }
}
