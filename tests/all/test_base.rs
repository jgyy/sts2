//! `base`: the same draws, sums and text as the original (tests/golden/base.json was written by the first version).

use serde_json::Value;
use sts2bot::base::{self, JsonExt, Random};

fn golden() -> Value {
    serde_json::from_str(include_str!("../golden/base.json")).unwrap()
}

#[test]
fn random_draws_what_the_record_drew() {
    let g = golden();
    for (seed, want) in g["random"].entries() {
        let mut r = Random::new(seed.parse::<i128>().unwrap());
        let w = want.list();
        assert_eq!(r.random(), w[0].float(), "seed {seed}");
        assert_eq!(r.randrange1(10), w[1].int());
        assert_eq!(r.randint(3, 9), w[2].int());
        assert_eq!(*r.choice(&[0, 1, 2, 3, 4, 5, 6]), w[3].int());
        assert_eq!(r.uniform(1.5, 9.25), w[4].float());
        assert_eq!(r.gauss(0.0, 12.0), w[5].float());
        assert_eq!(r.gauss(1.0, 2.0), w[6].float());
        assert_eq!(r.gauss(0.0, 1.0), w[7].float());
        let mut l: Vec<i64> = (0..20).collect();
        r.shuffle(&mut l);
        assert_eq!(l, w[8].list().iter().map(|x| x.int()).collect::<Vec<_>>());
        for (i, (n, k)) in [(30, 4), (10, 7), (100, 6), (200, 30)].into_iter().enumerate() {
            let pop: Vec<i64> = (0..n).collect();
            assert_eq!(r.sample(&pop, k), w[9 + i].list().iter().map(|x| x.int()).collect::<Vec<_>>(), "sample {n} {k}");
        }
        assert_eq!(r.randrange(5, 1 << 40), w[13].int());
        assert_eq!(r.getrandbits(64), w[14].as_u64().unwrap());
        assert_eq!(r.getrandbits(33), w[15].as_u64().unwrap());
        assert_eq!(r.random(), w[16].float());
    }
}

#[test]
fn sums_round_and_divide_as_recorded() {
    let g = golden();
    for row in g["sum"].list() {
        let xs: Vec<f64> = row[0].list().iter().map(|x| x.float()).collect();
        assert_eq!(base::fsum(xs.iter().copied()), row[1].float(), "{xs:?}");
        assert_eq!(base::fsum_from(0.5, xs.iter().copied()), row[2].float(), "{xs:?}");
    }
    for row in g["divmod"].list() {
        assert_eq!(base::floordiv(row[0].int(), row[1].int()), row[2].int());
        assert_eq!(base::modulo(row[0].int(), row[1].int()), row[3].int());
    }
    for row in g["fdivmod"].list() {
        assert_eq!(base::ffloordiv(row[0].float(), row[1].float()), row[2].float(), "{row}");
        assert_eq!(base::fmodulo(row[0].float(), row[1].float()), row[3].float(), "{row}");
    }
    for row in g["comb"].list() {
        assert_eq!(base::comb(row[0].int(), row[1].int()), row[2].int());
    }
    assert_eq!(base::max_by_key([(1, 'a'), (3, 'b'), (3, 'c')], |x| x.0), Some((3, 'b')));
    assert_eq!(base::min_by_key([(1, 'a'), (1, 'b'), (3, 'c')], |x| x.0), Some((1, 'a')));
}

#[test]
fn text_reads_as_the_record_wrote_it() {
    let g = golden();
    for row in g["repr"].list() {
        let x = row[0].float();
        assert_eq!(base::float_repr(x), row[1].s());
        assert_eq!(base::float_g(x), row[2].s(), "{x:e}");
        if x.abs() < 1e15 {
            assert_eq!(base::round(x), row[3].int(), "{x}");
        }
        assert_eq!(base::round_to(x, 2), row[4].float(), "{x}");
        assert_eq!(base::round_to(x, 1), row[5].float(), "{x}");
    }
    let obj: Value = serde_json::from_str(g["json"][0].s()).unwrap();
    assert_eq!(base::json_dumps(&obj), g["json"][0].s());
    assert_eq!(base::json_dumps_pretty(&obj, 1, true), g["json"][1].s());
    assert_eq!(base::Dumps { separators: Some((",", ":")), ..Default::default() }.dumps(&obj), g["json"][2].s());
    assert_eq!(base::Dumps { ensure_ascii: false, ..Default::default() }.dumps(&obj), g["json"][3].s());
    assert_eq!(base::repr_of(&serde_json::json!({"a": [1, 2.0, null, true, "it's", "q\""]})), g["json"][4].s());
    for row in g["str"].list() {
        assert_eq!(base::title(row[0].s()), row[1].s());
        assert_eq!(base::capitalize(row[0].s()), row[2].s());
        assert_eq!(base::str_repr(row[0].s()), row[3].s());
    }
}

#[test]
fn tunables_and_logging() {
    use sts2bot::logging::{self, Logger};
    sts2bot::tunables! { pub A: i64 = 3; pub B: f64 = 1.5; }
    static LOG: Logger = Logger::new("sts2bot.test");
    assert_eq!((A.get(), B.get()), (3, 1.5));
    {
        let _g = A.set(7);
        assert_eq!(A.get(), 7);
    }
    assert_eq!(A.get(), 3);
    assert!(set_tunable("B", 2.0) && !set_tunable("C", 1.0));
    assert_eq!(B.get(), 2.0);
    let ((), said) = logging::capture(|| sts2bot::log_warning!(LOG, "x {}", 1));
    assert_eq!(said[0].output(), "WARNING:sts2bot.test:x 1");
}
