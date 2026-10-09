use lopdf::{
    Document, Object, Stream,
    content::{Content, Operation},
    dictionary,
};
use std::path::Path;

pub(super) fn scanned_pdf(source: &Path, destination: &Path) {
    let image = image::open(source).unwrap().into_rgb8();
    let (width, height) = image.dimensions();
    let mut document = Document::with_version("1.5");
    let pages = document.new_object_id();
    let image = document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => width,
            "Height" => height, "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
        },
        image.into_raw(),
    ));
    let content = Content {
        operations: vec![
            Operation::new("q", vec![]),
            Operation::new(
                "cm",
                vec![
                    width.into(),
                    0.into(),
                    0.into(),
                    height.into(),
                    0.into(),
                    0.into(),
                ],
            ),
            Operation::new("Do", vec![Object::Name(b"Scan".to_vec())]),
            Operation::new("Q", vec![]),
        ],
    }
    .encode()
    .unwrap();
    let content = document.add_object(Stream::new(dictionary! {}, content));
    let page = document.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages, "Contents" => content,
        "MediaBox" => vec![0.into(), 0.into(), width.into(), height.into()],
        "Resources" => dictionary! { "XObject" => dictionary! { "Scan" => image } },
    });
    document.objects.insert(
        pages,
        dictionary! {
            "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
        }
        .into(),
    );
    let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    document.trailer.set("Root", catalog);
    document.save(destination).unwrap();
}
