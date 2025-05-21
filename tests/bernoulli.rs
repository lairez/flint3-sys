use std::{ffi::CStr, ptr::null_mut};

use flint_bindings::*;

#[test]
fn main() {
    unsafe {
        let mut x: fmpq = Default::default();
        let mut tot: fmpq = Default::default();
        fmpq_init(&mut x);
        fmpq_init(&mut tot);

        for n in 0..100 {
            bernoulli_fmpq_ui(&mut x, n);
            fmpq_add(&mut tot, &tot, &x);
            let s = CStr::from_ptr(fmpq_get_str(null_mut(), 10, &x))
                .to_str()
                .unwrap();
            println!("{s}");
        }

        let s = CStr::from_ptr(fmpq_get_str(null_mut(), 10, &tot))
            .to_str()
            .unwrap();

        assert_eq!(
            format!("{s}"),
            "129933074258434983784346358004759335381348403624655019424000199761232\
             46136040086804063311252859712599102622322902/1152783981972759212376551\
             073665878035"
        );

        fmpq_clear(&mut x);
        fmpq_clear(&mut tot);
    }
}
