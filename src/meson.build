# SPDX-FileCopyrightText: 2022  Emmanuele Bassi
# SPDX-License-Identifier: GPL-3.0-or-later

pkgdatadir = get_option('prefix') / get_option('datadir') / meson.project_name()

gnome.compile_resources('amberol',
  'amberol.gresource.xml',
  gresource_bundle: true,
  install: true,
  install_dir: pkgdatadir,
)

conf = configuration_data()
conf.set_quoted('VERSION', '@0@@1@'.format(meson.project_version(), version_suffix))
conf.set_quoted('GETTEXT_PACKAGE', 'amberol')
conf.set_quoted('LOCALEDIR', get_option('prefix') / get_option('localedir'))
conf.set_quoted('PKGDATADIR', pkgdatadir)
conf.set_quoted('APPLICATION_ID', application_id)
conf.set_quoted('PROFILE', get_option('profile'))

configure_file(
  input: 'config.rs.in',
  output: 'config.rs',
  configuration: conf
)

# Copy the config.rs output to the source directory.
run_command(
  'cp',
  join_paths(meson.project_build_root(), 'src', 'config.rs'),
  join_paths(meson.project_source_root(), 'src', 'config.rs'),
  check: true,
)

rust_sources = files(
  'application.rs',
  'audio/controller.rs',
  'audio/cover_cache.rs',
  'audio/gst_backend.rs',
  'audio/inhibit_controller.rs',
  'audio/mod.rs',
  'audio/mpris_controller.rs',
  'audio/player.rs',
  'audio/player_backend.rs',
  'audio/queue.rs',
  'audio/shuffle.rs',
  'audio/song.rs',
  'audio/state.rs',
  'audio/waveform_generator.rs',
  'config.rs',
  'cover_picture.rs',
  'drag_overlay.rs',
  'i18n.rs',
  'main.rs',
  'playback_control.rs',
  'playlist_view.rs',
  'queue_row.rs',
  'song_details.rs',
  'utils.rs',
  'volume_control.rs',
  'waveform_view.rs',
  'window.rs',
)

sources = [cargo_sources, rust_sources]

cargo_script = find_program(meson.project_source_root() / 'build-aux/cargo.sh')
cargo_release = custom_target(
  'cargo-build',
  build_by_default: true,
  input: sources,
  output: meson.project_name(),
  console: true,
  install: true,
  install_dir: get_option('bindir'),
  command: [
    cargo_script,
    meson.project_build_root(),
    meson.project_source_root(),
    '@OUTPUT@',
    get_option('buildtype'),
    meson.project_name(),
  ],
)
