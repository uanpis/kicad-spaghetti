# kicad-spaghetti
[kicad-spaghetti](https://github.com/uanpis/kicad-spaghetti.git) is a pcb post-processing plugin for [KiCad](https://kicad.org).
It models pcb tracks as networks of spring-connected masses, which act on eachother with tension and repulsion forces.

⚠️This plugin is still in early developement, so basic features are still missing.

## Usage
Make sure the KiCad API is enabled: ```Preferences > Plugins > ☑ Enable KiCad API```

...


## Building from source (all platforms)
1. Clone the repository

    ```
    git clone https://github.com/uanpis/kicad-spaghetti.git    
    ```
    ```
    cd kicad-spaghetti
    ```
2. Build and install
    ```
    python3 build.py --install
    ```
