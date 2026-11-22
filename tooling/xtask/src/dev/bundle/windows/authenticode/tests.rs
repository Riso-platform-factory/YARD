//! The refusal of a temporary folder whose volume keeps no access lists,
//! with the volume's answer faked.

use std::io;
use std::path::Path;

use super::keeps_access_lists;

#[test]
fn a_volume_without_access_lists_is_refused() {
    let temp = Path::new("T:\\temp");
    assert_eq!(keeps_access_lists(temp, &|_| Ok(true)), Ok(()));
    assert_eq!(
        keeps_access_lists(temp, &|_| Ok(false)),
        Err("the temporary folder T:\\temp is on a volume that keeps no access lists, so the certificate's file could not be private; set TEMP to a folder on an NTFS volume".to_owned())
    );
    assert_eq!(
        keeps_access_lists(temp, &|_| Err(io::Error::other("no such volume"))),
        Err("could not tell whether the temporary folder T:\\temp keeps access lists: no such volume".to_owned())
    );
}

#[test]
fn the_password_is_hidden_in_what_signtool_prints() {
    assert_eq!(
        super::hidden("bad password p4ss! given, p4ss! again\r\n", "p4ss!"),
        "bad password *** given, *** again\r\n"
    );
    assert_eq!(super::hidden("nothing to hide", "p4ss!"), "nothing to hide");
}
