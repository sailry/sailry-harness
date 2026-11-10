import 'package:flutter/widgets.dart';
import 'package:sailry_mobile/l10n/strings.dart' as product;
import 'generated/lookup.dart';

export 'package:sailry_mobile/l10n/strings.dart'
    show AppLocalizations, Translator;

/// Fixture translations are reachable only from the explicit preview entry point.
extension PreviewStrings on BuildContext {
  product.Translator get tr {
    final strings = Localizations.of<product.AppLocalizations>(
      this,
      product.AppLocalizations,
    );
    final locale = strings == null
        ? const Locale('zh')
        : Localizations.localeOf(this);
    return (key) =>
        lookupPreview(locale, key) ?? (strings?.tr(key) ?? product.tr(key));
  }
}

String tr(String key) =>
    lookupPreview(const Locale('zh'), key) ?? product.tr(key);
