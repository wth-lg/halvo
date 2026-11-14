use halvo_checkpoint::Walk;

use crate::error::{Error, PathRefusal};

impl From<Walk> for Error {
    fn from(walk: Walk) -> Error {
        match walk {
            Walk::Folder => Error::Folder,
            Walk::Outside => Error::Path(PathRefusal::Outside),
            Walk::Link => Error::Path(PathRefusal::Link),
            Walk::Directory => Error::Path(PathRefusal::Directory),
            Walk::NotFile => Error::Path(PathRefusal::NotFile),
            Walk::Name => Error::Path(PathRefusal::Name),
            Walk::Io(kind) => Error::Io(kind),
        }
    }
}
