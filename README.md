# transfers-indexer

This tool allows everyone to save the transfers of MAS made from an EOA forever using the Massa API and a MySQL database.

## Setup

### Database

You need to have a MySQL backend running to use this program. You can either use your own or, if you don't have one, we added a `docker-compose.yml` that run a MySQL in this repository.

When you have your database backend running make sure to configure the `.env` file at the root of this repository to match the credentials, host, etc... for your database.

### Massa node

You need to have a massa-node connected to this project in order to gather the transfers from the network.
This node needs to be launched using the feature `execution-trace`. The command to launch the node with the feature is : 
```
cargo run -r --features execution-trace
``` 
Make sure that the IP and port to the public API of your massa-node are matching in the `.env`

## Launch the transfers indexer

When everything is setup and your `.env` is updated you can launch the project using : 
```
cargo run -r
```

It will populate your database with all the transfers from the node and you will be able to access to the API.

If you stop your node, this program will be in error and so you need to restart it after your node is running again

## API documentation

- `/transfers/from/:address` Fetching all the transfers from a specified address. Example : `http://localhost:4444/transfers/from/AU12QxhhkrkGxewQ7vqkggsj81uchT1r3Qq1Hvn21rUXFQ94h1Nnv`

- `/transfers/to/:address` Fetching all the transfers to a specified address. Example : `http://localhost:4444/transfers/to/AU12QxhhkrkGxewQ7vqkggsj81uchT1r3Qq1Hvn21rUXFQ94h1Nnv`

- `/transfers/time_period/:start_date/:end_date` Fetching all the transfers between two dates. Example : `http://localhost:4444/transfers/time_period/2024-02-20T02:00:00Z/2024-02-21T00:00:00Z`