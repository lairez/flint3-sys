use flint3_sys::*;

unsafe fn aks_main(n: *const fmpz, a: ulong, r: ulong) -> bool {
    let mut modn_ctx: fmpz_mod_ctx_struct = Default::default();
    let mut xr1: fmpz_mod_poly_struct = Default::default();
    let mut xan: fmpz_mod_poly_struct = Default::default();
    let mut xna: fmpz_mod_poly_struct = Default::default();
    let mut t: fmpz_mod_poly_struct = Default::default();

    fmpz_mod_ctx_init(&mut modn_ctx, n);

    fmpz_mod_poly_init(&mut xr1, &modn_ctx);
    fmpz_mod_poly_init(&mut xan, &modn_ctx);
    fmpz_mod_poly_init(&mut xna, &modn_ctx);
    fmpz_mod_poly_init(&mut t, &modn_ctx);

    fmpz_mod_poly_set_coeff_si(&mut xr1, 0, -1, &modn_ctx);
    fmpz_mod_poly_set_coeff_ui(&mut xr1, r as slong, 1, &modn_ctx);

    fmpz_mod_poly_set_coeff_ui(&mut xan, 0, a, &modn_ctx);
    fmpz_mod_poly_set_coeff_ui(&mut xan, 1, 1, &modn_ctx);
    fmpz_mod_poly_powmod_fmpz_binexp(&mut xan, &xan, n, &xr1, &modn_ctx);

    fmpz_mod_poly_set_coeff_ui(&mut xna, 1, 1, &modn_ctx);
    fmpz_mod_poly_powmod_fmpz_binexp(&mut xna, &xna, n, &xr1, &modn_ctx);
    fmpz_mod_poly_set_ui(&mut t, a, &modn_ctx);
    fmpz_mod_poly_add(&mut xna, &xna, &t, &modn_ctx);

    let result = fmpz_mod_poly_equal(&xan, &xna, &modn_ctx) != 0;

    fmpz_mod_poly_clear(&mut xr1, &modn_ctx);
    fmpz_mod_poly_clear(&mut xan, &modn_ctx);
    fmpz_mod_poly_clear(&mut xna, &modn_ctx);
    fmpz_mod_poly_clear(&mut t, &modn_ctx);
    fmpz_mod_ctx_clear(&mut modn_ctx);

    result
}

unsafe fn multiplicative_order(a: ulong, m: ulong) -> ulong {
    let mut e = 1;
    loop {
        if n_powmod2(a, e as slong, m) == 1 {
            return e;
        }
        e += 1;
    }
}

unsafe fn is_perfect_power(n: *const fmpz) -> bool {
    let mut root: fmpz = Default::default();
    fmpz_init(&mut root);
    let result = fmpz_is_perfect_power(&mut root, n) != 0;
    fmpz_clear(&mut root);
    result
}

unsafe fn fmpz_is_prime_aks(n: *const fmpz) -> bool {
    if fmpz_cmp_ui(n, 1) <= 0 {
        return false;
    }

    if is_perfect_power(n) {
        return false;
    }

    let log2n = fmpz_clog_ui(n, 2) as ulong;
    let mut r = 2;

    loop {
        let n_mod_r = fmpz_fdiv_ui(n, r);
        if n_gcd(n_mod_r, r) == 1 {
            let ord = multiplicative_order(n_mod_r, r);
            if ord > log2n * log2n {
                break;
            }
        }
        r += 1;
    }

    let mut a = 2;
    while a <= r && fmpz_cmp_ui(n, a) > 0 {
        if fmpz_fdiv_ui(n, a) == 0 {
            return false;
        }
        a += 1;
    }

    if fmpz_cmp_ui(n, r) <= 0 {
        return true;
    }

    let bound = (n_sqrt(n_euler_phi(r)) + 1) * log2n;

    for a in 1..=bound {
        if !aks_main(n, a, r) {
            return false;
        }
    }

    true
}

unsafe fn set_decimal(n: *mut fmpz, value: &str) {
    let value = std::ffi::CString::new(value).unwrap();
    assert_eq!(fmpz_set_str(n, value.as_ptr(), 10), 0);
}

#[test]
fn aks_matches_flint_primality_for_small_integers() {
    unsafe {
        let mut n: fmpz = Default::default();
        fmpz_init(&mut n);
        fmpz_one(&mut n);

        while fmpz_cmp_ui(&n, 150) <= 0 {
            assert_eq!(
                fmpz_is_prime_aks(&n),
                fmpz_is_prime(&n) != 0,
                "wrong AKS result"
            );
            fmpz_add_ui(&mut n, &n, 1);
        }

        fmpz_clear(&mut n);
        flint_cleanup_master();
    }
}

#[test]
fn aks_checks_selected_examples() {
    unsafe {
        let mut n: fmpz = Default::default();
        fmpz_init(&mut n);

        set_decimal(&mut n, "97");
        assert!(fmpz_is_prime_aks(&n));

        set_decimal(&mut n, "341");
        assert!(!fmpz_is_prime_aks(&n));

        set_decimal(&mut n, "65537");
        assert!(fmpz_is_prime_aks(&n));

        fmpz_clear(&mut n);
        flint_cleanup_master();
    }
}
