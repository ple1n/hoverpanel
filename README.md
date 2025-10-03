
# Hoverpanel

work in progress.

![](https://github.com/user-attachments/assets/7f94c94d-a58d-44dc-a0f4-2dd725c9e705)

building

```sh
git clone https://github.com/ple1n/egui_tracing ../egui_tracing 
```

steps for importing dictionaries 

- Download archive from [IPFS](https://ipfs.io/ipfs/QmQP6BiPnwvYGuPGXKm4frRFSubA5jrwHXR9VeydvLwV25/)
- Extract files into a folder `/path/OpenMdicts/`
- `./target/debug/hoverpanel yaml -p "/path/OpenMdicts/*.yaml"` (works in fish)

```sh
$ hoverpanel stat
index loaded
function=load_file duration=178.062µs
Entries in database, 1618336. Unique words in index, 0.
$ hoverpanel build # takes a few minutes to build an index
index loaded
function=load_file duration=223.287µs
word set len 1442910
function=build_all duration=237.265975846s

index loaded
function=load_file duration=9.340071825s
function=build_index_from_db duration=259.211243985s
built, 1442910 words
```

fonts 

```sh
dnf install google-droid-fonts dejavusansmono-nerd-fonts # fedora
```

## Known bugs

- IME works sometimes and sometimes not. The state handling of wayland is probably faulty.
- XWayland causes mouse input to offset by 4 pixels, when an Xwayland window is below hoverpanel.
