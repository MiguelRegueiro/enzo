%bcond_with check
%global fallback_version 1.1.0
%global fallback_release 1

Name:           enzo
Version:        %{?enzo_version}%{!?enzo_version:%{fallback_version}}
Release:        %{?enzo_release}%{!?enzo_release:%{fallback_release}}%{?dist}
Summary:        Terminal video player

License:        MIT
URL:            https://github.com/MiguelRegueiro/enzo
Source0:        %{name}-%{version}.tar.gz
Source1:        vendor-%{version}.tar.zst

BuildRequires:  cargo-rpm-macros
BuildRequires:  cargo >= 1.96
BuildRequires:  rust >= 1.96
BuildRequires:  gcc
BuildRequires:  pkgconf-pkg-config
BuildRequires:  zstd
BuildRequires:  ffmpeg-free-devel
BuildRequires:  pulseaudio-libs-devel
BuildRequires:  freetype-devel
BuildRequires:  harfbuzz-devel
BuildRequires:  fribidi-devel
BuildRequires:  desktop-file-utils
Requires:       hicolor-icon-theme

%description
enzo is a terminal video player with synchronized audio, subtitles, and
full-color video output through the Kitty graphics protocol.

%prep
%autosetup -a 1
%cargo_prep -v vendor

%build
%cargo_build

%install
install -Dpm0755 target/rpm/%{name} %{buildroot}%{_bindir}/%{name}
install -Dpm0644 packaging/linux/%{name}.desktop %{buildroot}%{_datadir}/applications/%{name}.desktop
for size in 48 128 256 512; do
    install -Dpm0644 packaging/linux/icons/hicolor/${size}x${size}/apps/%{name}.png %{buildroot}%{_datadir}/icons/hicolor/${size}x${size}/apps/%{name}.png
done

%check
desktop-file-validate packaging/linux/%{name}.desktop
%if %{with check}
%cargo_test
%endif

%files
%license LICENSE
%doc README.md CHANGELOG.md
%{_bindir}/enzo
%{_datadir}/applications/%{name}.desktop
%{_datadir}/icons/hicolor/*/apps/%{name}.png

%changelog
* Wed Aug 12 2026 Miguel Regueiro <miguelpr4242@gmail.com> - 1.1.0-1
- Add folder playlists and improve HLS playback, remote media handling, and portable Linux releases

* Tue Aug 04 2026 Miguel Regueiro <miguelpr4242@gmail.com> - 1.0.0-1
- Initial COPR package
