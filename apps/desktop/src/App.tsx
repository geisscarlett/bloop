import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { invoke } from '@tauri-apps/api';
import { open } from '@tauri-apps/api/shell';
import { homeDir } from '@tauri-apps/api/path';
import { relaunch } from '@tauri-apps/api/process';
import { message, open as openDialog } from '@tauri-apps/api/dialog';
import { listen } from '@tauri-apps/api/event';
import * as tauriOs from '@tauri-apps/api/os';
import { getVersion } from '@tauri-apps/api/app';
import ClientApp from '../../../client/src/App';
import '../../../client/src/index.css';
import useKeyboardNavigation from '../../../client/src/hooks/useKeyboardNavigation';
import { getConfig } from '../../../client/src/services/api';
import TextSearch from './TextSearch';

// let askedToUpdate = false;
// let intervalId: number;

// listen(
//   'tauri://update-status',
//   async function (res: { payload: { status: string; error: Error | null } }) {
//     if (res.payload.status === 'DONE') {
//       const agreedToRestart = await ask(
//         `The installation was successful, do you want to restart the application now?`,
//         {
//           title: 'Ready to Restart',
//         },
//       );
//       if (agreedToRestart) {
//         relaunch();
//       }
//     } else if (res.payload.status === 'ERROR') {
//       await message(
//         'There was a problem updating bloop' +
//           (res.payload.error?.message || res.payload.error),
//         {
//           title: 'Update failed to install',
//           type: 'error',
//         },
//       );
//     }
//   },
// );

// const checkUpdateAndInstall = async (currentVersion: string) => {
//   try {
//     if (askedToUpdate) {
//       return;
//     }
//     const { shouldUpdate, manifest } = await checkUpdate();
//     if (shouldUpdate) {
//       const agreedToUpdate = await ask(
//         `bloop ${manifest?.version} is now available -- you have ${currentVersion}
//
// Would you like to install it now?
//
// Release notes:
// ${manifest?.body}`,
//         {
//           title: 'A new version of bloop is available!',
//         },
//       );
//       askedToUpdate = true;
//       if (intervalId) {
//         clearInterval(intervalId);
//       }
//       if (agreedToUpdate) {
//         installUpdate();
//       }
//     }
//   } catch (error) {
//     console.log(error);
//   }
// };

function App() {
  const [homeDirectory, setHomeDir] = useState('');
  const [indexFolder, setIndexFolder] = useState('');
  const [os, setOs] = useState({
    arch: '',
    type: '',
    platform: '',
    version: '',
  });
  const [release, setRelease] = useState('');
  const contentContainer = useRef<HTMLDivElement>(null);
  const [envConfig, setEnvConfig] = useState({});

  useEffect(() => {
    homeDir().then(setHomeDir);
    Promise.all([
      tauriOs.arch(),
      tauriOs.type(),
      tauriOs.platform(),
      tauriOs.version(),
      getVersion(),
    ]).then(([arch, type, platform, version, appVersion]) => {
      setOs({ arch, type, platform, version });
      setRelease(appVersion);
      // checkUpdateAndInstall(appVersion);
      // intervalId = window.setInterval(
      //   () => checkUpdateAndInstall(appVersion),
      //   1000 * 60 * 60,
      // );
    });
  }, []);

  const handleKeyEvent = useCallback((e: KeyboardEvent) => {
    if (
      (e.key === '=' || e.key === '-' || e.key === '0') &&
      (e.metaKey || e.ctrlKey)
    ) {
      const root = document.querySelector(':root');
      if (!root) {
        return;
      }
      const style = window
        .getComputedStyle(root, null)
        .getPropertyValue('font-size');
      const fontSize = parseFloat(style);

      (root as HTMLElement).style.fontSize =
        (e.key === '0' ? 16 : fontSize + (e.key === '=' ? 1 : -1)) + 'px';
    }
  }, []);
  useKeyboardNavigation(handleKeyEvent);

  useEffect(() => {
    setTimeout(() => getConfig().then(setEnvConfig), 1000); // server returns wrong tracking_id within first second
  }, []);

  const deviceContextValue = useMemo(
    () => ({
      openFolderInExplorer: (path: string) => {
        invoke('show_folder_in_finder', { path });
      },
      openLink: (path: string) => {
        open(path);
      },
      homeDir: homeDirectory,
      chooseFolder: openDialog,
      indexFolder,
      setIndexFolder,
      listen,
      os,
      invokeTauriCommand: invoke,
      release,
      apiUrl: 'http://127.0.0.1:7878/api',
      isRepoManagementAllowed: true,
      forceAnalytics: false,
      isSelfServe: false,
      envConfig,
      setEnvConfig,
      showNativeMessage: message,
      relaunch,
    }),
    [homeDirectory, indexFolder, os, release, envConfig],
  );
  return (
    <>
      <TextSearch contentRoot={contentContainer.current} />
      <div ref={contentContainer}>
        <ClientApp deviceContextValue={deviceContextValue} />
      </div>
    </>
  );
}

export default App;
