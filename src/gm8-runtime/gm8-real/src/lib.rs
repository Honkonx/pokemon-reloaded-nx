#![cfg_attr(not(feature = "std"), no_std)]

//! The OpenGMK numeric type, extracted without changing its arithmetic semantics
//! so portable GameMaker runtime code can use it on `core` + `alloc` targets.

extern crate alloc;

use core::{
    cmp::Ordering,
    fmt,
    ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Rem, Sub, SubAssign},
};
use serde::{Deserialize, Serialize};

mod float {
    #[cfg(feature = "std")]
    pub fn floor(x: f64) -> f64 {
        x.floor()
    }
    #[cfg(not(feature = "std"))]
    pub fn floor(x: f64) -> f64 {
        libm::floor(x)
    }

    #[cfg(feature = "std")]
    pub fn sin(x: f64) -> f64 {
        x.sin()
    }
    #[cfg(not(feature = "std"))]
    pub fn sin(x: f64) -> f64 {
        libm::sin(x)
    }
    #[cfg(feature = "std")]
    pub fn cos(x: f64) -> f64 {
        x.cos()
    }
    #[cfg(not(feature = "std"))]
    pub fn cos(x: f64) -> f64 {
        libm::cos(x)
    }
    #[cfg(feature = "std")]
    pub fn tan(x: f64) -> f64 {
        x.tan()
    }
    #[cfg(not(feature = "std"))]
    pub fn tan(x: f64) -> f64 {
        libm::tan(x)
    }
    #[cfg(feature = "std")]
    pub fn asin(x: f64) -> f64 {
        x.asin()
    }
    #[cfg(not(feature = "std"))]
    pub fn asin(x: f64) -> f64 {
        libm::asin(x)
    }
    #[cfg(feature = "std")]
    pub fn acos(x: f64) -> f64 {
        x.acos()
    }
    #[cfg(not(feature = "std"))]
    pub fn acos(x: f64) -> f64 {
        libm::acos(x)
    }
    #[cfg(feature = "std")]
    pub fn atan(x: f64) -> f64 {
        x.atan()
    }
    #[cfg(not(feature = "std"))]
    pub fn atan(x: f64) -> f64 {
        libm::atan(x)
    }
    #[cfg(feature = "std")]
    pub fn atan2(x: f64, y: f64) -> f64 {
        x.atan2(y)
    }
    #[cfg(not(feature = "std"))]
    pub fn atan2(x: f64, y: f64) -> f64 {
        libm::atan2(x, y)
    }
    #[cfg(feature = "std")]
    pub fn exp(x: f64) -> f64 {
        x.exp()
    }
    #[cfg(not(feature = "std"))]
    pub fn exp(x: f64) -> f64 {
        libm::exp(x)
    }
    #[cfg(feature = "std")]
    pub fn ln(x: f64) -> f64 {
        x.ln()
    }
    #[cfg(not(feature = "std"))]
    pub fn ln(x: f64) -> f64 {
        libm::log(x)
    }
    #[cfg(feature = "std")]
    pub fn log2(x: f64) -> f64 {
        x.log2()
    }
    #[cfg(not(feature = "std"))]
    pub fn log2(x: f64) -> f64 {
        libm::log2(x)
    }
    #[cfg(feature = "std")]
    pub fn log10(x: f64) -> f64 {
        x.log10()
    }
    #[cfg(not(feature = "std"))]
    pub fn log10(x: f64) -> f64 {
        libm::log10(x)
    }
    #[cfg(feature = "std")]
    pub fn log(x: f64, base: f64) -> f64 {
        x.log(base)
    }
    #[cfg(not(feature = "std"))]
    pub fn log(x: f64, base: f64) -> f64 {
        libm::log(x) / libm::log(base)
    }
    #[cfg(feature = "std")]
    pub fn sqrt(x: f64) -> f64 {
        x.sqrt()
    }
    #[cfg(not(feature = "std"))]
    pub fn sqrt(x: f64) -> f64 {
        libm::sqrt(x)
    }
    #[cfg(feature = "std")]
    pub fn ceil(x: f64) -> f64 {
        x.ceil()
    }
    #[cfg(not(feature = "std"))]
    pub fn ceil(x: f64) -> f64 {
        libm::ceil(x)
    }
    #[cfg(feature = "std")]
    pub fn trunc(x: f64) -> f64 {
        x.trunc()
    }
    #[cfg(not(feature = "std"))]
    pub fn trunc(x: f64) -> f64 {
        libm::trunc(x)
    }
    #[cfg(feature = "std")]
    pub fn rem_euclid(x: f64, y: f64) -> f64 {
        x.rem_euclid(y)
    }
    #[cfg(not(feature = "std"))]
    pub fn rem_euclid(x: f64, y: f64) -> f64 {
        let r = libm::fmod(x, y);
        if r < 0.0 { r + y.abs() } else { r }
    }
    #[cfg(feature = "std")]
    pub fn fract(x: f64) -> f64 {
        x.fract()
    }
    #[cfg(not(feature = "std"))]
    pub fn fract(x: f64) -> f64 {
        x - libm::trunc(x)
    }
}

/// A transparent wrapper for f64 which intended to emulate Double type from Delphi.
#[derive(Copy, Clone, Default, Serialize, Deserialize)]
#[repr(transparent)]
pub struct Real(f64);

/// The default way to round as defined by IEEE 754 - nearest, ties to even.
pub fn ieee_round(real: f64) -> f64 {
    let int = float::floor(real);
    let diff = real - int;
    if diff < 0.5 {
        int
    } else if diff > 0.5 {
        int + 1.0
    } else {
        int + (int as i64 & 1) as f64
    }
}

impl Real {
    #[inline(always)]
    pub fn sin(self) -> Self {
        Self(float::sin(self.0))
    }

    #[inline(always)]
    pub fn cos(self) -> Self {
        Self(float::cos(self.0))
    }

    #[inline(always)]
    pub fn tan(self) -> Self {
        Self(float::tan(self.0))
    }

    #[inline(always)]
    pub fn arcsin(self) -> Self {
        Self(float::asin(self.0))
    }

    #[inline(always)]
    pub fn arccos(self) -> Self {
        Self(float::acos(self.0))
    }

    #[inline(always)]
    pub fn arctan(self) -> Self {
        Self(float::atan(self.0))
    }

    #[inline(always)]
    pub fn arctan2(self, other: Self) -> Self {
        Self(float::atan2(self.0, other.0))
    }

    #[inline(always)]
    pub fn exp(self) -> Self {
        Self(float::exp(self.0))
    }

    #[inline(always)]
    pub fn ln(self) -> Self {
        Self(float::ln(self.0))
    }

    #[inline(always)]
    pub fn log2(self) -> Self {
        Self(float::log2(self.0))
    }

    #[inline(always)]
    pub fn log10(self) -> Self {
        Self(float::log10(self.0))
    }

    #[inline(always)]
    pub fn logn(self, other: Self) -> Self {
        Self(float::log(self.0, other.0))
    }

    #[inline(always)]
    pub fn sqrt(self) -> Self {
        Self(float::sqrt(self.0))
    }
}

impl Add for Real {
    type Output = Self;

    #[inline(always)]
    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl Sub for Real {
    type Output = Self;

    #[inline(always)]
    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

impl Mul for Real {
    type Output = Self;

    #[inline(always)]
    fn mul(self, other: Self) -> Self {
        Self(self.0 * other.0)
    }
}

impl Div for Real {
    type Output = Self;

    #[inline(always)]
    fn div(self, other: Self) -> Self {
        Self(self.0 / other.0)
    }
}

impl From<u8> for Real {
    #[inline(always)]
    fn from(i: u8) -> Self {
        Self(f64::from(i))
    }
}

impl From<i32> for Real {
    #[inline(always)]
    fn from(i: i32) -> Self {
        Self(f64::from(i))
    }
}

impl From<u32> for Real {
    #[inline(always)]
    fn from(i: u32) -> Self {
        Self(f64::from(i))
    }
}

impl From<u64> for Real {
    #[inline(always)]
    fn from(i: u64) -> Self {
        Self(i as f64)
    }
}

impl From<f64> for Real {
    #[inline(always)]
    fn from(f: f64) -> Self {
        Self(f)
    }
}

impl From<Real> for f64 {
    #[inline(always)]
    fn from(real: Real) -> Self {
        real.0
    }
}

impl fmt::Debug for Real {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Display for Real {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl AddAssign for Real {
    #[inline(always)]
    fn add_assign(&mut self, other: Self) {
        *self = *self + other
    }
}

impl SubAssign for Real {
    #[inline(always)]
    fn sub_assign(&mut self, other: Self) {
        *self = *self - other
    }
}

impl MulAssign for Real {
    #[inline(always)]
    fn mul_assign(&mut self, other: Self) {
        *self = *self * other
    }
}

impl DivAssign for Real {
    #[inline(always)]
    fn div_assign(&mut self, other: Self) {
        *self = *self / other
    }
}

impl Rem for Real {
    type Output = Self;

    #[inline(always)]
    fn rem(self, other: Self) -> Self {
        Self(self.0 % other.0)
    }
}

impl Neg for Real {
    type Output = Self;

    #[inline(always)]
    fn neg(self) -> Self {
        Self(-self.0)
    }
}

impl PartialEq for Real {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl Eq for Real {}

impl PartialOrd for Real {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

impl core::iter::Sum<Self> for Real {
    #[inline]
    fn sum<I>(iter: I) -> Self
    where
        I: Iterator<Item = Self>,
    {
        iter.fold(Self(0.0), Self::add)
    }
}

impl Real {
    /// The lenience between values when compared.
    pub const CMP_EPSILON: Self = Self(1e-13);

    #[inline(always)]
    pub fn into_inner(self) -> f64 {
        self.0
    }

    #[inline(always)]
    pub fn as_ref(&self) -> &f64 {
        &self.0
    }

    #[inline(always)]
    pub fn as_mut_ref(&mut self) -> &mut f64 {
        &mut self.0
    }

    #[inline(always)]
    pub fn to_i8(self) -> i8 {
        self.0 as i64 as i8
    }

    #[inline(always)]
    pub fn to_i32(self) -> i32 {
        self.0 as i64 as i32
    }

    #[inline(always)]
    pub fn to_u32(self) -> u32 {
        self.0 as i64 as u32
    }

    #[inline(always)]
    pub fn to_u64(self) -> u64 {
        self.0 as u64
    }

    #[inline(always)]
    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    #[inline(always)]
    pub fn round(self) -> Self {
        // So-called "banker's rounding", identical to Math.Round() from Delphi.
        Self(ieee_round(self.0))
    }

    #[inline(always)]
    pub fn floor(self) -> Self {
        Self(float::floor(self.0))
    }

    #[inline(always)]
    pub fn floor_towards_zero(self) -> Self {
        Self(float::floor(self.0.abs()) * self.0.signum())
    }

    #[inline(always)]
    pub fn ceil(self) -> Self {
        Self(float::ceil(self.0))
    }

    #[inline(always)]
    pub fn trunc(self) -> Self {
        Self(float::trunc(self.0))
    }

    #[inline(always)]
    pub fn fract(self) -> Self {
        Self(float::fract(self.0))
    }

    #[inline(always)]
    pub fn to_radians(self) -> Self {
        Self(self.0.to_radians())
    }

    #[inline(always)]
    pub fn to_degrees(self) -> Self {
        Self(self.0.to_degrees())
    }

    #[inline(always)]
    pub fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    #[inline(always)]
    pub fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }

    #[inline(always)]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self(self.0.clamp(min.0, max.0))
    }

    #[inline(always)]
    pub fn rem_euclid(self, other: Self) -> Self {
        Self(float::rem_euclid(self.0, other.0))
    }

    #[inline]
    pub fn cmp_nan_first(&self, other: &Self) -> Ordering {
        if self.0.is_nan() {
            if other.0.is_nan() {
                return Ordering::Equal
            }
            return Ordering::Less
        }
        if other.0.is_nan() {
            return Ordering::Greater
        }
        self.partial_cmp(other).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::Real;
    use core::f64::consts::PI;

    fn assert_near(actual: Real, expected: f64) {
        assert!((actual.into_inner() - expected).abs() <= 1e-14, "{actual:?} != {expected:?}");
    }

    #[test]
    fn add() {
        assert_eq!(Real(3.0), Real(1.0) + Real(2.0));
    }

    #[test]
    fn sub() {
        assert_eq!(Real(1.0), Real(3.0) - Real(2.0));
    }

    #[test]
    fn mul() {
        assert_eq!(Real(6.0), Real(3.0) * Real(2.0));
        assert_eq!(Real(-2.0), Real(2.0) * Real(-1.0));
    }

    #[test]
    fn div() {
        assert_eq!(Real(3.0), Real(6.0) / Real(2.0));
        assert_eq!(Real(-1.0), Real(2.0) / Real(-2.0));
    }

    #[test]
    fn rem() {
        assert_eq!(Real(1.0), Real(7.0) % Real(3.0));
        assert_eq!(Real(0.75), Real(160.5) % Real(2.25));
    }

    #[test]
    fn neg() {
        assert_eq!(-Real(3.0), Real(-3.0));
    }

    #[test]
    fn prec_19() {
        const INCREMENT: Real = Real(0.2);
        let mut x = INCREMENT;
        let target = Real(19.0);
        loop {
            x += INCREMENT;
            if (x - target).abs() <= Real::CMP_EPSILON {
                break
            } else if x > target {
                panic!();
            }
        }
    }

    #[test]
    fn lt() {
        assert_eq!(Real(3.0) < Real(4.0), true);
        assert_eq!(Real(3.0) < Real(3.0), false);
        assert_eq!(Real(-3.0) < Real(-4.0), false);
        assert_eq!(Real(0.3) < Real(0.1) + Real(0.2), true);
    }

    #[test]
    fn le() {
        assert_eq!(Real(3.0) <= Real(4.0), true);
        assert_eq!(Real(3.0) <= Real(3.0), true);
        assert_eq!(Real(-3.0) <= Real(-4.0), false);
        assert_eq!(Real(0.3) <= Real(0.1) + Real(0.2), true);
    }

    #[test]
    fn gt() {
        assert_eq!(Real(4.0) > Real(3.0), true);
        assert_eq!(Real(3.0) > Real(3.0), false);
        assert_eq!(Real(-4.0) > Real(-3.0), false);
        assert_eq!(Real(0.1) + Real(0.2) > Real(0.3), true);
    }

    #[test]
    fn ge() {
        assert_eq!(Real(4.0) >= Real(3.0), true);
        assert_eq!(Real(3.0) >= Real(3.0), true);
        assert_eq!(Real(-4.0) >= Real(-3.0), false);
        assert_eq!(Real(0.1) + Real(0.2) >= Real(0.3), true);
    }

    #[test]
    fn round() {
        assert_eq!(Real(0.0), Real(0.0).round());
        assert_eq!(Real(3.0), Real(3.14).round());
        assert_eq!(Real(10.0), Real(9.9).round());
        assert_eq!(Real(0.0), Real(-0.4).round());
        assert_eq!(Real(-13.0), Real(-13.37).round());
        for i in 0..1000 {
            let Real(rounded) = Real(f64::from(i) + 0.5).round();
            assert_eq!(0.0, rounded.fract());
            assert_eq!(0, (rounded as i32) % 2);
        }
    }

    #[test]
    fn sin() {
        assert_eq!(Real(PI / 2.0).sin(), Real(1.0));
    }

    #[test]
    fn cos() {
        assert_eq!(Real(PI).cos(), Real(-1.0));
    }

    #[test]
    fn tan() {
        assert_near(Real(PI).tan(), 0.0);
    }

    #[test]
    fn arcsin() {
        assert_eq!(Real(0.8).arcsin(), Real(0.9272952180016123));
    }

    #[test]
    fn arccos() {
        assert_near(Real(0.8).arccos(), 0.6435011087932844);
    }

    #[test]
    fn arctan() {
        assert_eq!(Real(123.4).arctan(), Real(1.5626927764648464));
    }

    #[test]
    fn arctan2() {
        assert_near(Real(5.0).arctan2(Real(8.1)), 0.5530314441506405);
        assert_near(Real(8.1).arctan2(Real(5.0)), 1.0177648826442560);
    }

    #[test]
    fn exp() {
        assert_near(Real(3.1).exp(), 22.19795128144164);
    }

    #[test]
    fn ln() {
        assert_eq!(Real(core::f64::consts::E).ln(), Real(1.0));
    }

    #[test]
    fn log2() {
        assert_eq!(Real(2.0).log2(), Real(1.0));
        assert_eq!(Real(8.0).log2(), Real(3.0));
    }

    #[test]
    fn log10() {
        assert_eq!(Real(10.0).log10(), Real(1.0));
        assert_eq!(Real(1000.0).log10(), Real(3.0));
    }

    #[test]
    fn logn() {
        assert_near(Real(343.0).logn(Real(7.0)), 3.0);
    }

    #[test]
    fn sqrt() {
        assert_eq!(Real(9.0).sqrt(), Real(3.0));
    }

    #[test]
    fn to_degrees() {
        assert_eq!(Real(core::f64::consts::PI).to_degrees(), Real(180.0));
    }

    #[test]
    fn to_radians() {
        assert_eq!(Real(180.0).to_radians(), Real(core::f64::consts::PI));
    }
}
