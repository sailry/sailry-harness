import {View, div} from 'gpui-kit';
import {title, directory} from './labels.js';

export default class Probe extends View {
  render() {
    const titles = [
      ['user@host: ~/Projects/test2', 'test2'],
      ['user@server:/srv/my project/', 'my project'],
      ['/Volumes/Data/测试', '测试'],
      ['C:\\Users\\user\\project', 'project'],
      ['\\\\server\\share\\project', 'project'],
      ['user@host:~', '~'], ['/', '/'],
      ['user@host:', null], ['user@host', null], [' ', null], [null, null],
      ...['npm run dev', 'Build user@example.com', 'https://example.com/app',
        'Release: staging', 'vim main.rs'].map(value => [value, value]),
    ];
    const directories = [
      ['file://remote-host/home/user/my%20project', 'my project'],
      ['file://localhost/tmp/%E6%B5%8B%E8%AF%95', '测试'],
      ['file://localhost/tmp/bad%ZZ', null],
      ['/srv/project/', 'project'], ['', null], [null, null],
    ];
    for (const [format, cases] of [[title, titles], [directory, directories]]) {
      for (const [input, expected] of cases) {
        const actual = format(input);
        if (actual !== expected) throw new Error(`Unexpected label for ${input}: ${actual}`);
      }
    }
    return div().id('title-rules-passed');
  }
}
