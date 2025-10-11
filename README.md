# Forwarding Translation Proxy

A Minecraft Java Edition proxy intended for connecting old Minecraft Version servers, that only support legacy bungeecord forwarding, to Velocity Modern Forwarding Networks.

> [!IMPORTANT]
> This proxy will not work for you out of the box, please consult the [Proxy Compatibility](#proxy-compatibility) section first.

> [!NOTE]
> This project is, even though not tested for all versions, intended to work for Minecraft versions 1.7.2 to 1.12.2, as these are supported by Velocity, but don't support Modern Forwarding.
> As I am not testing all of these versions, please report any issues and I will get to fixing them.

## How to use

> [!CAUTION]
> The connection to this Proxy may be more secure through the use of the Modern Forwarding protocol,
> but the connection from this proxy to the backend server is still insecure.
> Please make sure you have everything configured properly before you let people connect

You currently need to compile the project yourself. I will look into providing pre-compiled binaries and docker containers later.

1. Run the application, it will shut down at first and generate a `Config.toml` file, which can also be seen in this repository.
2. Fill out the config options, this should be pretty self-explanatory, but here is an overview:
    - `listen_address`: You can configure the address this proxy is reachable at here, this is what your Modern Proxy forwards the connections to.
    - `backend_address`: The address of your backend server, this is your Minecraft server that only supports legacy bungeecord forwarding.
    - `forwarding_secret`: This is the secret found in `forwarding.secret` in your Velocity configuration. You can also configure this through the environment variable `FORWARDING_SECRET`.
    - `trusted_ips`: This is a list of ip addresses that connections are allowed from, this should be the address of your Modern Proxy(s). Although not recommended, you can leave this empty to allow all connections if you know what you are doing or for development.
    - `log_level`: The logging verbosity of this proxy. Should not need to be adjusted unless you are developing or reporting an error.
3. Point your [*MODIFIED*](#proxy-compatibility) Modern Proxy to whatever ip address and port you configured in `listen_address`.
4. Make sure your backend server is configured to accept legacy bungeecord connections and is running at the specified `backend_address`.
5. Start the application again, it should now be running and listening for connections, connecting your legacy server to it and your modern proxy.

## Proxy Compatibility

This application will most likely not work for you out of the box, I only tried this with [Velocity](https://papermc.io/software/velocity), I suspect other proxies like [Gate](https://gate.minekube.com/) will have similar issues. Your Proxy will definitely need to support the modern forwarding protocol for this to work!

### Modifying Velocity

I recommend compiling Velocity yourself, the changes that need to be done are minor and easy to understand. You can inspect them in the [diff](https://github.com/PaperMC/Velocity/compare/dev/3.0.0...GrandmasterB42:Velocity:dev/3.0.0) and apply them yourself to whatever version of Velocity you want to use, it is probably similar for most versions.

## Resources I used

Thanks to all the awesome people that made the following resources available!

- [Minecraft Protocol Wiki](https://minecraft.wiki/w/Java_Edition_protocol)
- [A Blog Post I found](https://ashhhleyyy.dev/blog/2022-06-27-velocity-information-forwarding) by [this person](https://ashhhleyyy.dev/)
- [FabricProxy-Lite](https://github.com/OKTW-Network/FabricProxy-Lite/)
- [BungeeForge](https://github.com/caunt/BungeeForge)
