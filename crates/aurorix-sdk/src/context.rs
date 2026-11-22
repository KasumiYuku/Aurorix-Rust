use aurorix_core::{Scene, Target};

use crate::error::SdkError;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Context {
    pub scene: Scene,
    pub target: Target,
    pub user: Option<String>,
}

impl Context {
    pub fn new(scene: Scene, target: Target) -> Self {
        Self {
            scene,
            target,
            user: None,
        }
    }

    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }
}

pub trait Extract: Sized {
    fn extract(ctx: &Context) -> Result<Self, SdkError>;
}

pub fn extract<T: Extract>(ctx: &Context) -> Result<T, SdkError> {
    T::extract(ctx)
}

impl Extract for Scene {
    fn extract(ctx: &Context) -> Result<Self, SdkError> {
        Ok(ctx.scene)
    }
}

impl Extract for Target {
    fn extract(ctx: &Context) -> Result<Self, SdkError> {
        Ok(ctx.target.clone())
    }
}

impl Extract for String {
    fn extract(ctx: &Context) -> Result<Self, SdkError> {
        ctx.user
            .clone()
            .ok_or_else(|| SdkError::Invalid("上下文里没有用户标识".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group() -> Context {
        Context::new(
            Scene::Group,
            Target::new(Scene::Group, "g1").with_user("u1"),
        )
    }

    #[test]
    fn scene_and_target_are_always_available() {
        let ctx = group();
        assert_eq!(Scene::extract(&ctx), Ok(Scene::Group));
        assert_eq!(extract::<Scene>(&ctx), Ok(Scene::Group));
        let target = Target::extract(&ctx).expect("目标要有");
        assert_eq!(target.id, "g1");
        assert_eq!(target.scene, Scene::Group);
    }

    #[test]
    fn user_requires_the_optional_field() {
        let ctx = group();
        assert_eq!(
            String::extract(&ctx),
            Err(SdkError::Invalid("上下文里没有用户标识".to_string())),
            "缺用户时要报错"
        );
        assert_eq!(
            String::extract(&ctx).unwrap_err().to_string(),
            "参数不合法: 上下文里没有用户标识"
        );
        let ctx = ctx.with_user("openid-1");
        assert_eq!(String::extract(&ctx), Ok("openid-1".to_string()));
        assert_eq!(extract::<String>(&ctx), Ok("openid-1".to_string()));
    }

    #[test]
    fn chained_context_keeps_scene_and_target() {
        let ctx = Context::new(Scene::C2c, Target::new(Scene::C2c, "u9")).with_user("u9");
        assert_eq!(ctx.scene, Scene::C2c);
        assert_eq!(ctx.target.to_string(), "c2c/u9");
        assert_eq!(ctx.user, Some("u9".to_string()));
        assert_eq!(
            Target::extract(&ctx).map(|target| target.id),
            Ok("u9".to_string())
        );
    }
}
