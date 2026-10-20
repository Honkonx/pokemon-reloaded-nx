//! `f32`/`f64` math methods that `core` lacks (they live in `std`). Brought into scope by the
//! prelude so `x.sin()`, `x.floor()`, ... keep compiling without `std`; implemented with `libm`.

pub trait F64Ext: Sized {
    fn sin(self) -> f64;
    fn cos(self) -> f64;
    fn tan(self) -> f64;
    fn asin(self) -> f64;
    fn acos(self) -> f64;
    fn atan(self) -> f64;
    fn atan2(self, other: f64) -> f64;
    fn sinh(self) -> f64;
    fn cosh(self) -> f64;
    fn tanh(self) -> f64;
    fn exp(self) -> f64;
    fn exp2(self) -> f64;
    fn ln(self) -> f64;
    fn log10(self) -> f64;
    fn log2(self) -> f64;
    fn log(self, base: f64) -> f64;
    fn powi(self, n: i32) -> f64;
    fn powf(self, n: f64) -> f64;
    fn sqrt(self) -> f64;
    fn cbrt(self) -> f64;
    fn floor(self) -> f64;
    fn ceil(self) -> f64;
    fn round(self) -> f64;
    fn trunc(self) -> f64;
    fn fract(self) -> f64;
    fn hypot(self, other: f64) -> f64;
    fn mul_add(self, a: f64, b: f64) -> f64;
    fn rem_euclid(self, rhs: f64) -> f64;
}

impl F64Ext for f64 {
    fn sin(self) -> f64 { libm::sin(self) }
    fn cos(self) -> f64 { libm::cos(self) }
    fn tan(self) -> f64 { libm::tan(self) }
    fn asin(self) -> f64 { libm::asin(self) }
    fn acos(self) -> f64 { libm::acos(self) }
    fn atan(self) -> f64 { libm::atan(self) }
    fn atan2(self, other: f64) -> f64 { libm::atan2(self, other) }
    fn sinh(self) -> f64 { libm::sinh(self) }
    fn cosh(self) -> f64 { libm::cosh(self) }
    fn tanh(self) -> f64 { libm::tanh(self) }
    fn exp(self) -> f64 { libm::exp(self) }
    fn exp2(self) -> f64 { libm::exp2(self) }
    fn ln(self) -> f64 { libm::log(self) }
    fn log10(self) -> f64 { libm::log10(self) }
    fn log2(self) -> f64 { libm::log2(self) }
    fn log(self, base: f64) -> f64 { libm::log(self) / libm::log(base) }
    fn powi(self, n: i32) -> f64 { libm::pow(self, n as f64) }
    fn powf(self, n: f64) -> f64 { libm::pow(self, n) }
    fn sqrt(self) -> f64 { libm::sqrt(self) }
    fn cbrt(self) -> f64 { libm::cbrt(self) }
    fn floor(self) -> f64 { libm::floor(self) }
    fn ceil(self) -> f64 { libm::ceil(self) }
    fn round(self) -> f64 { libm::round(self) }
    fn trunc(self) -> f64 { libm::trunc(self) }
    fn fract(self) -> f64 { self - libm::trunc(self) }
    fn hypot(self, other: f64) -> f64 { libm::hypot(self, other) }
    fn mul_add(self, a: f64, b: f64) -> f64 { libm::fma(self, a, b) }
    fn rem_euclid(self, rhs: f64) -> f64 {
        let r = self % rhs;
        if r < 0.0 { r + rhs.abs() } else { r }
    }
}

pub trait F32Ext: Sized {
    fn log2(self) -> f32;
    fn log10(self) -> f32;
    fn exp2(self) -> f32;
    fn asin(self) -> f32;
    fn acos(self) -> f32;
    fn atan(self) -> f32;
    fn cbrt(self) -> f32;
    fn mul_add(self, a: f32, b: f32) -> f32;
    fn rem_euclid(self, rhs: f32) -> f32;
    fn sin(self) -> f32;
    fn cos(self) -> f32;
    fn tan(self) -> f32;
    fn atan2(self, other: f32) -> f32;
    fn exp(self) -> f32;
    fn ln(self) -> f32;
    fn powi(self, n: i32) -> f32;
    fn powf(self, n: f32) -> f32;
    fn sqrt(self) -> f32;
    fn floor(self) -> f32;
    fn ceil(self) -> f32;
    fn round(self) -> f32;
    fn trunc(self) -> f32;
    fn fract(self) -> f32;
    fn hypot(self, other: f32) -> f32;
}

impl F32Ext for f32 {
    fn log2(self) -> f32 { libm::log2f(self) }
    fn log10(self) -> f32 { libm::log10f(self) }
    fn exp2(self) -> f32 { libm::exp2f(self) }
    fn asin(self) -> f32 { libm::asinf(self) }
    fn acos(self) -> f32 { libm::acosf(self) }
    fn atan(self) -> f32 { libm::atanf(self) }
    fn cbrt(self) -> f32 { libm::cbrtf(self) }
    fn mul_add(self, a: f32, b: f32) -> f32 { libm::fmaf(self, a, b) }
    fn rem_euclid(self, rhs: f32) -> f32 {
        let r = self % rhs;
        if r < 0.0 { r + rhs.abs() } else { r }
    }
    fn sin(self) -> f32 { libm::sinf(self) }
    fn cos(self) -> f32 { libm::cosf(self) }
    fn tan(self) -> f32 { libm::tanf(self) }
    fn atan2(self, other: f32) -> f32 { libm::atan2f(self, other) }
    fn exp(self) -> f32 { libm::expf(self) }
    fn ln(self) -> f32 { libm::logf(self) }
    fn powi(self, n: i32) -> f32 { libm::powf(self, n as f32) }
    fn powf(self, n: f32) -> f32 { libm::powf(self, n) }
    fn sqrt(self) -> f32 { libm::sqrtf(self) }
    fn floor(self) -> f32 { libm::floorf(self) }
    fn ceil(self) -> f32 { libm::ceilf(self) }
    fn round(self) -> f32 { libm::roundf(self) }
    fn trunc(self) -> f32 { libm::truncf(self) }
    fn fract(self) -> f32 { self - libm::truncf(self) }
    fn hypot(self, other: f32) -> f32 { libm::hypotf(self, other) }
}
