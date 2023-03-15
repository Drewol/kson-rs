use std::rc::Rc;

use three_d::{Context, Geometry, Material, Object, Program};

pub struct RuscMat {
    program: Rc<Program>,
    context: Context,
}

impl Object for RuscMat {
    fn render(&self, camera: &three_d::Camera, lights: &[&dyn three_d::Light]) {}

    fn material_type(&self) -> three_d::MaterialType {
        todo!()
    }
}

impl Geometry for RuscMat {
    fn render_with_material(
        &self,
        material: &dyn Material,
        camera: &three_d::Camera,
        lights: &[&dyn three_d::Light],
    ) {
        todo!()
    }

    fn render_with_post_material(
        &self,
        material: &dyn three_d::PostMaterial,
        camera: &three_d::Camera,
        lights: &[&dyn three_d::Light],
        color_texture: Option<three_d::ColorTexture>,
        depth_texture: Option<three_d::DepthTexture>,
    ) {
        todo!()
    }

    fn aabb(&self) -> three_d::AxisAlignedBoundingBox {
        todo!()
    }
}
