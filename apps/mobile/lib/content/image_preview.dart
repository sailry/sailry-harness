import 'package:flutter/material.dart';

import '../l10n/strings.dart';
import '../ui/icons.dart';
import '../ui/loading.dart';

/// The same provider is reused by the thumbnail and full-screen viewer.
class ImageThumbnail extends StatelessWidget {
  const ImageThumbnail({super.key, required this.image, this.name, this.save});

  final ImageProvider image;
  final String? name;
  final Future<void> Function()? save;

  @override
  Widget build(BuildContext context) => Semantics(
    button: true,
    label: name == null ? tr('imagePreview') : null,
    child: InkWell(
      onTap: () => showImagePreview(context, image, name: name, save: save),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxHeight: 240),
        child: Image(
          image: image,
          semanticLabel: name,
          fit: BoxFit.contain,
          errorBuilder: (_, _, _) => Text(tr('conversationImageFailed')),
        ),
      ),
    ),
  );
}

/// Never resolve execution-node paths against the phone's filesystem.
Widget uriImage(Uri uri) {
  try {
    if (uri.scheme == 'http' || uri.scheme == 'https') {
      return ImageThumbnail(image: NetworkImage(uri.toString()));
    }
    if (uri.scheme == 'data') {
      final data = uri.data;
      if (data != null && data.mimeType.startsWith('image/')) {
        return ImageThumbnail(image: MemoryImage(data.contentAsBytes()));
      }
    }
  } on FormatException {
    // Invalid image data should not prevent rendering the rest of the document.
  }
  return Text(tr('conversationImageFailed'));
}

Future<void> showImagePreview(
  BuildContext context,
  ImageProvider image, {
  String? name,
  Future<void> Function()? save,
}) => showDialog<void>(
  context: context,
  useSafeArea: false,
  barrierColor: Theme.of(context).colorScheme.scrim.withValues(alpha: .95),
  builder: (_) => _ImagePreview(image: image, name: name, save: save),
);

class _ImagePreview extends StatefulWidget {
  const _ImagePreview({required this.image, this.name, this.save});
  final ImageProvider image;
  final String? name;
  final Future<void> Function()? save;

  @override
  State<_ImagePreview> createState() => _ImagePreviewState();
}

class _ImagePreviewState extends State<_ImagePreview> {
  bool _saving = false;
  String? _error;

  Future<void> _save() async {
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      await widget.save!();
    } catch (_) {
      if (mounted) setState(() => _error = tr('fileSaveFailed'));
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) => Dialog.fullscreen(
    backgroundColor: Colors.transparent,
    child: IconTheme(
      data: const IconThemeData(color: Colors.white),
      child: DefaultTextStyle(
        style: Theme.of(
          context,
        ).textTheme.bodyMedium!.copyWith(color: Colors.white),
        child: SafeArea(
          child: Column(
            children: [
              Row(
                children: [
                  IconButton(
                    tooltip: tr('close'),
                    onPressed: () => Navigator.pop(context),
                    icon: const AppIcon('close'),
                  ),
                  Expanded(
                    child: Text(
                      widget.name ?? tr('imagePreview'),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                  if (widget.save != null)
                    TextButton(
                      style: TextButton.styleFrom(
                        foregroundColor: Colors.white,
                      ),
                      onPressed: _saving ? null : _save,
                      child: Text(tr('save')),
                    ),
                ],
              ),
              if (_error != null)
                Padding(
                  padding: const EdgeInsets.all(12),
                  child: Text(_error!),
                ),
              Expanded(
                child: LoadingOverlay(
                  loading: _saving,
                  child: ImageViewer(image: widget.image),
                ),
              ),
            ],
          ),
        ),
      ),
    ),
  );
}

/// Shared zoom and pan interaction for file pages and conversation lightboxes.
class ImageViewer extends StatefulWidget {
  const ImageViewer({super.key, required this.image});
  final ImageProvider image;

  @override
  State<ImageViewer> createState() => _ImageViewerState();
}

class _ImageViewerState extends State<ImageViewer> {
  final _transform = TransformationController();
  Offset _doubleTap = Offset.zero;

  @override
  void dispose() {
    _transform.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => GestureDetector(
    onDoubleTapDown: (details) => _doubleTap = details.localPosition,
    onDoubleTap: () {
      _transform.value = _transform.value.getMaxScaleOnAxis() > 1
          ? Matrix4.identity()
          : (Matrix4.identity()
              ..translateByDouble(-_doubleTap.dx, -_doubleTap.dy, 0, 1)
              ..scaleByDouble(2, 2, 1, 1));
    },
    child: InteractiveViewer(
      transformationController: _transform,
      minScale: 1,
      maxScale: 8,
      child: SizedBox.expand(
        child: Image(
          image: widget.image,
          fit: BoxFit.contain,
          errorBuilder: (_, _, _) =>
              Center(child: Text(tr('conversationImageFailed'))),
        ),
      ),
    ),
  );
}
