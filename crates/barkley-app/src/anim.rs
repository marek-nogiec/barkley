//! Small Core Animation helpers: spring pop, fades, staggered row reveals.
//! Everything degrades to a plain fade when Reduce Motion is on.

use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::{NSAnimationContext, NSWorkspace};
use objc2_foundation::{NSString, NSValue};
use objc2_quartz_core::{
    CABasicAnimation, CACurrentMediaTime, CALayer, CAMediaTiming, CAMediaTimingFunction, CASpringAnimation,
    CATransform3D, CATransform3DIdentity, NSValueCATransform3DAdditions, kCAFillModeBackwards, kCAFillModeForwards,
};

pub fn reduce_motion() -> bool {
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

/// Fast start, gentle settle. Used for frame changes.
pub fn ease_out() -> Retained<CAMediaTimingFunction> {
    CAMediaTimingFunction::functionWithControlPoints(0.2, 0.9, 0.25, 1.0)
}

/// Run `changes` inside an animation group; `done` runs when it finishes.
pub fn group(duration: f64, changes: impl Fn() + 'static, done: impl Fn() + 'static) {
    let changes = RcBlock::new(move |ctx: NonNull<NSAnimationContext>| {
        let ctx = unsafe { ctx.as_ref() };
        ctx.setDuration(duration);
        ctx.setAllowsImplicitAnimation(true);
        ctx.setTimingFunction(Some(&ease_out()));
        changes();
    });
    let done = RcBlock::new(done);
    NSAnimationContext::runAnimationGroup_completionHandler(&changes, Some(&done));
}

/// Uniform scale around (`x`, `y`) in the layer's own coordinates
/// (AppKit layers anchor at their origin).
fn scale_about(s: f64, x: f64, y: f64) -> CATransform3D {
    let to_origin = CATransform3D::new_translation(-x, -y, 0.0);
    let back = CATransform3D::new_translation(x, y, 0.0);
    to_origin.concat(CATransform3D::new_scale(s, s, 1.0)).concat(back)
}

fn transform_value(t: CATransform3D) -> Retained<NSValue> {
    unsafe { NSValue::valueWithCATransform3D(t) }
}

fn key(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

/// Springy scale-up from `from_scale` to 1 around (`x`, `y`).
pub fn pop_in(layer: &CALayer, from_scale: f64, x: f64, y: f64) {
    layer.removeAnimationForKey(&key("barkley.hide"));
    let spring = CASpringAnimation::animationWithKeyPath(Some(&key("transform")));
    spring.setMass(1.0);
    spring.setStiffness(420.0);
    spring.setDamping(24.0); // a touch under critical (~41): one soft overshoot
    spring.setInitialVelocity(0.0);
    unsafe {
        spring.setFromValue(Some(&transform_value(scale_about(from_scale, x, y))));
        spring.setToValue(Some(&transform_value(CATransform3DIdentity)));
    }
    spring.setDuration(spring.settlingDuration());
    spring.setFillMode(unsafe { kCAFillModeBackwards });
    layer.addAnimation_forKey(&spring, Some(&key("barkley.pop")));
}

/// Shrink to `to_scale` and hold there until the next `pop_in`.
pub fn shrink_out(layer: &CALayer, to_scale: f64, x: f64, y: f64, duration: f64) {
    layer.removeAnimationForKey(&key("barkley.pop"));
    let a = CABasicAnimation::animationWithKeyPath(Some(&key("transform")));
    unsafe {
        a.setFromValue(Some(&transform_value(CATransform3DIdentity)));
        a.setToValue(Some(&transform_value(scale_about(to_scale, x, y))));
    }
    a.setDuration(duration);
    a.setTimingFunction(Some(&CAMediaTimingFunction::functionWithControlPoints(0.4, 0.0, 1.0, 1.0)));
    a.setFillMode(unsafe { kCAFillModeForwards });
    a.setRemovedOnCompletion(false);
    layer.addAnimation_forKey(&a, Some(&key("barkley.hide")));
}

/// Fade + slide `layer` in after `delay` seconds; invisible until it starts.
pub fn reveal(layer: &CALayer, delay: f64, dy: f64) {
    let begin = layer.convertTime_fromLayer(CACurrentMediaTime(), None) + delay;
    let fade = CABasicAnimation::animationWithKeyPath(Some(&key("opacity")));
    unsafe {
        fade.setFromValue(Some(&objc2_foundation::NSNumber::new_f64(0.0)));
        fade.setToValue(Some(&objc2_foundation::NSNumber::new_f64(1.0)));
    }
    let slide = CABasicAnimation::animationWithKeyPath(Some(&key("transform")));
    unsafe {
        slide.setFromValue(Some(&transform_value(CATransform3D::new_translation(0.0, dy, 0.0))));
        slide.setToValue(Some(&transform_value(CATransform3DIdentity)));
    }
    for a in [&fade, &slide] {
        a.setBeginTime(begin);
        a.setDuration(0.22);
        a.setTimingFunction(Some(&ease_out()));
        a.setFillMode(unsafe { kCAFillModeBackwards });
    }
    layer.addAnimation_forKey(&fade, Some(&key("barkley.reveal.opacity")));
    layer.addAnimation_forKey(&slide, Some(&key("barkley.reveal.slide")));
}
