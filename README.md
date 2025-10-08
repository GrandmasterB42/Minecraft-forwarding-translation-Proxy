> [!CAUTION]
> This project is under active development and will not work out of the box for you

# Forwarding Translation Proxy

> [!IMPORTANT]
> This project does not work with the normal version of Velocity, as it assumes, that older server versions do not know about certain message types. I am looking into what to do about that

> [!WARNING]
> This project has *NOT* been extensively tested, please report any issues you find

A Minecraft Java Edition proxy intended for connecting old Minecraft Version servers, that only support legacy bungeecord forwarding, to Velocity Modern Forwarding Networks.

> [!CAUTION]
> The Proxy may be more secure through the use of the Modern Forwarding protocol, but the connection from this proxy to the backend server is still insecure.
> Please make sure you have everything configured properly before you let people connect

## How to use

You currently need to compile the project yourself. I will look into providing pre-compiled binaries and docker containers later.

1. Run the application, it will shut down at first and generate a `Config.toml` file, which can also be seen in this repository.
2. Fill out the config options, this should be pretty self-explanatory, but here is an overview:
    - `listen_address`: You can configure the address this proxy is reachable at here, this is what your Modern Proxy forwards the connections to.
    - `backend_address`: The address of your backend server, this is your Minecraft server that only supports legacy bungeecord forwarding.
    - `forwarding_secret`: This is the secret found in `forwarding.secret` in your Velocity configuration. You can also configure this through the environment variable `FORWARDING_SECRET`.
    - `trusted_ips`: This is a list of ip addresses that connections are allowed from, this should be the address of your Modern Proxy(s). Although not recommended, you can leave this empty to allow all connections if you know what you are doing or for development.
    - `log_level`: The logging verbosity of this proxy. Should not need to be adjusted unless you are developing or reporting an error.
3. Point your Modern Proxy to whatever ip address and port you configured in `listen_address`.
4. Make sure your backend server is configured to accept legacy bungeecord connections and is running at the specified `backend_address`.
5. Start the application again, it should now be running and listening for connections, connecting your legacy server to it and your modern proxy.

## Resources I used

Thanks to all the awesome people that made the following resources available!

- [Minecraft Protocol Wiki](https://minecraft.wiki/w/Java_Edition_protocol)
- [A Blog Post I found](https://ashhhleyyy.dev/blog/2022-06-27-velocity-information-forwarding) by [this person](https://ashhhleyyy.dev/)
- [FabricProxy-Lite](https://github.com/OKTW-Network/FabricProxy-Lite/)
- [BungeeForge](https://github.com/caunt/BungeeForge)
