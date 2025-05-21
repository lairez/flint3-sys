use std::ffi::CStr;

use flint3_sys::*;

#[test]
fn main() {
    unsafe {
        let mut res: fmpz_poly_struct = std::mem::zeroed();
        let d = -99;
        flint_set_num_threads(2);

        fmpz_poly_init(&mut res);

        acb_modular_hilbert_class_poly(&mut res, d);

        let s = fmpz_poly_get_str_pretty(&res, c"x".as_ptr());
        let s = CStr::from_ptr(s).to_str().unwrap();

        assert_eq!(
            format!("{s}"),
            "x^2+37616060956672*x-56171326053810176",
        );
    }
}
