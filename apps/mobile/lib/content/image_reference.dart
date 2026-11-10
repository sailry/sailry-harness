import 'dart:convert';

/// `sailry-image:<tool-call-id>/<index>` names an image returned earlier in the
/// conversation. Client resolves it; this only reads the Markdown address.
typedef ImageReference = ({String call, int index});

ImageReference? imageReference(Uri uri) {
  if (uri.scheme != 'sailry-image' || uri.hasAuthority) return null;
  final path = uri.path;
  final slash = path.lastIndexOf('/');
  if (slash <= 0) return null;
  final digits = path.substring(slash + 1);
  if (!RegExp(r'^(0|[1-9][0-9]*)$').hasMatch(digits)) return null;
  final String call;
  try {
    call = Uri.decodeComponent(path.substring(0, slash));
  } on Object {
    return null;
  }
  if (call.isEmpty || utf8.encode(call).length > 512) return null;
  final index = int.tryParse(digits);
  return index == null ? null : (call: call, index: index);
}
