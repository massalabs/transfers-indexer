# Transfers Indexer

The Transfers Indexer is a powerful tool designed for archiving MAS token transfers from Externally Owned Accounts (EOA) on the Massa blockchain. It utilizes the API exposed by the [Massa node](https://github.com/massalabs/massa) for data collection and stores this information in a MySQL database.

## Prerequisite

- **Massa Node with Execution Trace**: You must have access to a Massa node compiled with the `execution-trace` feature or run the source code with `cargo run -r --features execution-trace`.

## Setup Instructions

To start the indexer, execute:

```sh
cargo run -r
```

This command launches the indexing process, capturing and storing transfer data from the Massa blockchain.

## API Documentation

For a detailed overview of the API endpoints, including query parameters, request examples, and response structures, please refer to our [Swagger API documentation](transfers-indexer.yml).


## FAQ

### How to Use a Remote Massa Node

To configure the indexer to connect to a remote Massa node, update the `.env` file with the remote node's IP and public port:

```
MASSA_NODE_API_IP="remote_node_ip"
MASSA_NODE_API_PUBLIC_PORT="remote_node_port"
```

### How to Configure the API to Listen on a Different Port or Network Interface

To change the network interface or port the API listens to, modify the `INDEXER_API` value in the `.env` file:

```
INDEXER_API="desired_ip:desired_port"
```

### Using a Remote MySQL Database

If you prefer using a remote MySQL database instead of a local or Docker-based instance, specify the database URL in the `.env` file:

```
DATABASE_URL='mysql://user:password@remote_host:3306/massa_transfers'
```

### Changing Database Credentials

To change the database credentials, adjust the `DATABASE_URL` in the `.env` file to reflect the new user and password:

```
DATABASE_URL='mysql://new_user:new_password@localhost:3306/massa_transfers'
```

This configuration allows the indexer to connect to the specified MySQL database using the provided credentials.