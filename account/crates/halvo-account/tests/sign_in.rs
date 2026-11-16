use halvo_account::sign_in::SignIn;
use halvo_secrets::Secret;
use halvo_wire::request::{AccountId, Credential};

fn secret(bytes: &[u8]) -> Secret {
    Secret::new(bytes.to_vec()).expect("a secret")
}

#[test]
fn a_sign_in_is_kept_as_the_id_one_space_and_the_credential() {
    let mut random = [0; 48];
    for (at, byte) in random.iter_mut().enumerate() {
        *byte = at as u8 * 5;
    }
    let made = SignIn::from_random(&random);
    let kept = made.to_secret();
    let text = std::str::from_utf8(kept.expose()).expect("ASCII");
    assert_eq!(text.len(), SignIn::SECRET_LEN);
    assert_eq!(SignIn::SECRET_LEN, 101);
    let (id, credential) = text.split_at(36);
    assert!(AccountId::parse(id).is_some());
    assert_eq!(id, made.account().as_str());
    assert_eq!(&credential[..1], " ");
    assert!(Credential::parse(&credential[1..]).is_some());
    assert_eq!(&credential[1..], made.credential().as_str());
    let mut id_bytes = [0; 16];
    id_bytes.copy_from_slice(&random[..16]);
    let mut credential_bytes = [0; 32];
    credential_bytes.copy_from_slice(&random[16..]);
    assert_eq!(id, AccountId::from_random(id_bytes).as_str());
    assert_eq!(
        &credential[1..],
        Credential::from_random(credential_bytes).as_str()
    );
}

#[test]
fn a_kept_sign_in_reads_back_as_itself() {
    for seed in [0u8, 1, 127, 255] {
        let made = SignIn::from_random(&[seed; 48]);
        let read = SignIn::from_secret(&made.to_secret()).expect("its own secret");
        assert_eq!(read.account().as_str(), made.account().as_str());
        assert_eq!(read.credential().as_str(), made.credential().as_str());
    }
}

#[test]
fn two_kinds_of_random_bytes_make_two_sign_ins() {
    let one = SignIn::from_random(&[1; 48]);
    let mut other_bytes = [1; 48];
    other_bytes[47] = 2;
    let other = SignIn::from_random(&other_bytes);
    assert_eq!(one.account().as_str(), other.account().as_str());
    assert_ne!(one.credential().as_str(), other.credential().as_str());
    other_bytes[0] = 9;
    let third = SignIn::from_random(&other_bytes);
    assert_ne!(one.account().as_str(), third.account().as_str());
}

#[test]
fn a_secret_of_another_form_is_no_sign_in() {
    let good = SignIn::from_random(&[7; 48]).to_secret().expose().to_vec();
    let mut forms: Vec<Vec<u8>> = vec![
        good[..100].to_vec(),
        [good.clone(), b"0".to_vec()].concat(),
        [good.clone(), b"\n".to_vec()].concat(),
        [b" ".to_vec(), good[..100].to_vec()].concat(),
        vec![b' '; 101],
        vec![0xc3; 101],
    ];
    for at in [0, 8, 14, 19, 35, 36, 37, 70, 100] {
        let mut changed = good.clone();
        changed[at] = match changed[at] {
            b'-' => b'0',
            b' ' => b'\t',
            _ => b'G',
        };
        forms.push(changed);
    }
    let mut upper = good.clone();
    upper[50] = upper[50].to_ascii_uppercase();
    if upper != good {
        forms.push(upper);
    } else {
        upper[50] = b'A';
        forms.push(upper);
    }
    for form in forms {
        assert!(
            SignIn::from_secret(&secret(&form)).is_none(),
            "{:?}",
            String::from_utf8_lossy(&form)
        );
    }
}
