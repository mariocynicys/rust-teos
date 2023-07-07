## Running `teosd` in a docker container
A `teos` image can be built from the Dockerfile located in `docker`. You can create the image by running:

	cd rust-teos
	docker build -f docker/Dockerfile -t teos .
	
Then you can create a container by running:

	docker run -it -e <ENV_VARIABLES> teos
	
Notice that ENV variables are optional, if unset the corresponding default setting is used. The following ENVs are available:

```
- API_BIND=<teos_api_hostname>
- API_PORT=<teos_api_port>
- RPC_BIND=<teos_rpc_hostname>
- RPC_PORT=<teos_rpc_port>
- BTC_NETWORK=<btc_network>
- BTC_RPC_CONNECT=<btc_node_hostname>
- BTC_RPC_PORT=<btc_node_port>
- BTC_RPC_USER=<btc_rpc_username>
- BTC_RPC_PASSWORD=<btc_rpc_password>
- DATA_DIR=<teos_data_dir>
- DEBUG=<debug_bool>
- DEPS_DEBUG=<deps_debug_bool>
- OVERWRITE_KEY=<overwrite_key_bool>
- FORCE_UPDATE=<force_update_bool>
```

You may also want to run docker with a volume, so you can have data persistence in `teosd` databases and keys.
If so, run:

    docker volume create teos-data
    
And add the the mount parameter to `docker run`:

    --mount source=teos-data,target=/root/.teos

If you are running `teosd` and `bitcoind` in the same machine, continue reading for how to create the container based on your OS.

### `bitcoind` running on the same machine (UNIX)
The easiest way to run both together in he same machine using UNIX is to set the container to use the host network.
	
For example, if both `teosd` and `bitcoind` are running on default settings, run
    
```
docker run --network=host \
  --name teos \
  --mount source=teos-data,target=/root/.teos \
  -e BTC_RPC_CONNECT=host.docker.internal \
  -e BTC_RPC_USER=<rpc username> \
  -e BTC_RPC_PASSWORD=<rpc password> \
  -it teos
```

Notice that you may still need to set your RPC authentication details, since, hopefully, your credentials won't match the `teosd` defaults.

### `bitcoind` running on the same machine (OSX or Windows)

Docker for OSX and Windows does not allow to use the host network (nor to use the `docker0` bridge interface). To workaround this
you can use the special `host.docker.internal` domain.

```
docker run -p 9814:9814 \
  --name teos \
  --mount source=teos-data,target=/root/.teos \
  -e BTC_RPC_CONNECT=host.docker.internal \
  -e BTC_RPC_USER=<rpc username> \
  -e BTC_RPC_PASSWORD=<rpc password> \
  -e API_BIND=0.0.0.0 \
  -it teos
```

Notice that we also needed to add `API_BIND=0.0.0.0` to bind the API to all interfaces of the container.
Otherwise it will bind to `localhost` and we won't be able to send requests to the tower from the host.

### Using `teos-cli`

With this set up, there are two ways you can use `teos-cli`

You can open an `sh` shell using `docker exec`:

`docker exec -it <CONTAINER NAME> sh`

Then begin issuing whatever `teos-cli` commands you want.

Secondly, you can try using teos-cli remotely following the instructions in [the main README](https://github.com/talaia-labs/rust-teos/blob/master/README.md#running-teos-cli-remotely).
