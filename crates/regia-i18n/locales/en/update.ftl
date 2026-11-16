# The app's native dialogs of an update, and the CLI's `self-update`.

update-title = { -product }: update
update-offer = Version { $version } of { -product } is ready. Install it now? The app will close and start again.
update-failed = The update was not installed: { $reason }
update-current = { -product } { $version } is the latest version.
update-available = Version { $version } is available: run `self-update` without `--check` to install it.
update-installed = Installed version { $version }: the next command runs it.
update-with-the-app = This copy of the command line came with the app: the app installs its updates.
