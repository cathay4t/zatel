# Zatel -- Network Config Tool

Still work in progress

## Goal

* Easy for user to configure the network to fit their complex needs.
* Easy for developer to contribute.

## Design
* Nispor providing __full__ network status of kernel for all interfaces
  or single interface.
* Nispor taking __single__ interface __full__ configuration to apply.
* Zatel provides:
    * Point in time network state query, no caching.
    * Unified network state both from user space and kernel, so that
      user could get full picture of certain interface.
    * Smart network management with minimum prerequisite knowledge.
        * Ordering the network creation/management to kernel/user space.
        * Wraping complex layout to simple options.
    * Front-end plugins for variable developer friendly APIs.
        * NetworkManager daemon only provide unix socket interface
          for querying, changing and notification.
        * Frondends(DBUS, varlink, etc) just wrapping unix socket to their
          own tech.
        * Easy to creating binding for other develop lanagurages.
    * Plugins for DHCP, 802.1x, OVS, VPN, WIFI, DNS and etc.
        * Socket communication to child process of plugin allowing
          plugin been written in any language it likes.
        * A dedicate plugin for backwards compatibility.


## What should zatel core do and not do

* `zateld` -- daemon
    * Provide plugin management.
    * Provide socket for plugin and API communication.
    * Load config file.
    * Do not need to understand the detailed schema of each interface.
    * Do not need to verify whether plugin is doing its works.
    * Checkpoint support.

* `libztl` -- API for communication to daemon

* `ztl` -- CLI tools

## What should zatel plugins do and not do

## Implementation detail

* `zatel-varlink` -- plugin for varlink interface
* `ztl` -- CLI tool

## TODO

* Daemon for unix socket commands
* Plugin interface
