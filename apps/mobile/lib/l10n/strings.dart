import 'package:flutter/widgets.dart';

import 'generated/app_localizations.dart';
import 'generated/lookup.dart';

export 'generated/app_localizations.dart' show AppLocalizations;

typedef Translator = String Function(String key);

final _defaultStrings = lookupAppLocalizations(const Locale('zh'));

/// Adapts dynamic menu/status keys to Flutter's generated, typed resources.
extension LocalizedStrings on AppLocalizations {
  String tr(String key) =>
      lookupString(this, key) ??
      (throw ArgumentError.value(key, 'key', 'Missing localization'));
}

extension LocalizedContext on BuildContext {
  Translator get tr =>
      (Localizations.of<AppLocalizations>(this, AppLocalizations) ??
              _defaultStrings)
          .tr;
}

bool hasLocalization(String key) => lookupString(_defaultStrings, key) != null;

/// Default copy for isolated fixtures without the application delegate.
String tr(String key) => _defaultStrings.tr(key);
