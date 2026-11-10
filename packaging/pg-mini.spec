Name:           pg-mini
Version:        %{pkg_version}
Release:        1%{?dist}
Summary:        Lightweight native PostgreSQL manager
License:        MIT
URL:            https://github.com/iamaer4fa/pg-mini
Source0:        pg-mini
Source1:        pg-mini.desktop
Source2:        pg-mini.svg
Source3:        pg-mini.png

Requires:       glibc
Requires:       libgcc
Requires:       mesa-libGL
Requires:       libwayland-client
Requires:       libX11
Requires:       libxcb
Requires:       libxkbcommon

%description
pg-mini is a lightweight native desktop client for browsing PostgreSQL
databases, running SQL queries, and monitoring PostgreSQL high availability.

%install
install -Dpm0755 %{SOURCE0} %{buildroot}%{_bindir}/pg-mini
install -Dpm0644 %{SOURCE1} %{buildroot}%{_datadir}/applications/pg-mini.desktop
install -Dpm0644 %{SOURCE2} %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/pg-mini.svg
install -Dpm0644 %{SOURCE3} %{buildroot}%{_datadir}/icons/hicolor/256x256/apps/pg-mini.png

%files
%{_bindir}/pg-mini
%{_datadir}/applications/pg-mini.desktop
%{_datadir}/icons/hicolor/scalable/apps/pg-mini.svg
%{_datadir}/icons/hicolor/256x256/apps/pg-mini.png
