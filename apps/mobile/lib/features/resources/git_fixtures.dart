import '../../l10n/strings.dart';

class GitPreview {
  GitPreview(this.branch, {Translator translate = tr})
    : branches = {branch, 'main', 'feature/sign-in', 'fix/layout'},
      history = [
        (id: 'sample-2', title: translate('gitHistoryLayout'), branch: branch),
        (id: 'sample-1', title: translate('gitHistoryInit'), branch: 'main'),
      ];

  String branch;
  final Set<String> branches;
  final List<({String id, String title, String branch})> history;
  bool staged = false;
  bool committed = false;
  String file = 'login.css';
  int ahead = 1;
  int behind = 1;
  int sequence = 3;
}

const gitDiffs = <String, ({String path, List<String> lines})>{
  'login.css': (
    path: 'src/styles/login.css',
    lines: [
      ' .login-form {',
      '-  gap: 12px;',
      '+  gap: 20px;',
      '   display: flex;',
      '   flex-direction: column;',
      ' }',
      ' ',
      '+.login-input:focus-visible {',
      '+  outline: 2px solid',
      '+    var(--focus-ring);',
      '+  outline-offset: 3px;',
      '+}',
      ' ',
      ' .login-button {',
      '-  height: 36px;',
      '+  min-height: 44px;',
      ' }',
    ],
  ),
  'Login.tsx': (
    path: 'src/pages/Login.tsx',
    lines: [
      ' export function Login() {',
      '   return (',
      '-    <div className="form">',
      '+    <form className="login-form">',
      '       <EmailInput />',
      '       <PasswordInput />',
      '+      <SubmitButton />',
      '+    </form>',
      '   );',
      ' }',
    ],
  ),
  'login.test.ts': (
    path: 'src/tests/login.test.ts',
    lines: [
      '+it("preserves keyboard focus",',
      '+  async () => {',
      '+    await openLogin();',
      '+    await pressTab();',
      '+    expect(email).toBeFocused();',
      '+  }',
      '+);',
    ],
  ),
};
