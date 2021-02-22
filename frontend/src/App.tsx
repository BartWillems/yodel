import React from "react";
import ReconnectingWebSocket from "reconnecting-websocket";
import { Url } from "url";
import {
  Container,
  Header,
  Table,
  Grid,
  Form,
  Segment,
} from "semantic-ui-react";
import dayjs from "dayjs";
import relativeTime from "dayjs/plugin/relativeTime";
import { SemanticToastContainer, toast } from "react-semantic-toasts";
import "react-semantic-toasts/styles/react-semantic-alert.css";

dayjs.extend(relativeTime);

const WebsocketURI =
  process.env.REACT_APP_WS_URL ||
  ((window.location.protocol === "https:" && "wss://") || "ws://") +
    window.location.host +
    "/ws";

const API_URI = process.env.REACT_APP_API_URL;

interface Location {
  name: String;
  path: String;
}

interface Job {
  url: Url;
  title?: String;
  location: Location;
  startedOn: Date;
}

const JobList = ({ jobs }: { jobs: Job[] }) => {
  return (
    <Table celled>
      <Table.Header>
        <Table.Row>
          <Table.HeaderCell>Video</Table.HeaderCell>
          <Table.HeaderCell>Started On</Table.HeaderCell>
        </Table.Row>
      </Table.Header>

      <Table.Body>
        {jobs.map((job, index) => {
          return (
            <Table.Row key={index}>
              <Table.Cell>
                <a href={`${job.url}`}>{job.title ? job.title : job.url}</a>
              </Table.Cell>
              <Table.Cell>{dayjs(job.startedOn).fromNow()}</Table.Cell>
            </Table.Row>
          );
        })}
      </Table.Body>
    </Table>
  );
};

const CreateJobForm = () => {
  const [loading, setLoading] = React.useState<boolean>(false);
  const [url, setUrl] = React.useState<String>("");
  const [location, setLocation] = React.useState<String>("akkefietjes");

  const handleSubmit = () => {
    setLoading(true);
    fetch(`${API_URI}/jobs`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        url,
        location,
      }),
    })
      .then((resp) => {
        if (resp.ok) {
          setUrl("");
          return;
        }

        resp.json().then((res) => {
          let title = "Unexpected job startup error!";
          let type: "error" | "warning" = "error";
          if (resp.statusText === "Conflict") {
            title = "Conflict: job was already requested";
            type = "warning";
          }
          if (resp.statusText === "Unprocessable Entity") {
            title = "Invalid Video Requested";
          }
          toast({
            type: type,
            title,
            description: res,
            time: 0,
          });
        });
      })
      .catch((err) => {
        console.error(err);
      })
      .finally(() => {
        setLoading(false);
      });
  };

  return (
    <Form onSubmit={() => handleSubmit()} loading={loading}>
      <Form.Input
        placeholder="URL"
        name="name"
        label="Video URL"
        value={url}
        onChange={(ev, val) => {
          setUrl(val.value);
        }}
        required
      />
      <Form.Select
        fluid
        label="Location"
        defaultValue="akkefietjes"
        options={[{ key: "1", text: "Akkefietjes", value: "akkefietjes" }]}
        onChange={(ev, val) => setLocation(val.value!.toString()!)}
      />
      <Form.Button>Submit</Form.Button>
    </Form>
  );
};

function App() {
  const [pendingJobs, setPendingJobs] = React.useState<Job[]>([]);
  const [completedJobs, setCompletedJobs] = React.useState<Job[]>([]);
  const [connected, setConnected] = React.useState(false);

  React.useEffect(() => {
    fetch(`${API_URI}/jobs`)
      .then((resp) => resp.json() as Promise<Job[]>)
      .then((data) => {
        setPendingJobs(data);
      })
      .catch((err) => {
        console.error(err);
      });
  }, []);

  React.useEffect(() => {
    fetch(`${API_URI}/completed-jobs`)
      .then((resp) => resp.json() as Promise<Job[]>)
      .then((data) => {
        setCompletedJobs(data);
      })
      .catch((err) => {
        console.error(err);
      });
  }, []);

  React.useEffect(() => {
    const socket = new ReconnectingWebSocket(WebsocketURI);

    socket.onmessage = (update) => {
      const message = JSON.parse(update.data);

      console.log(message);

      if (message.PendingJobs) {
        setPendingJobs(message.PendingJobs);
      }

      if (message.CompletedJobs) {
        setCompletedJobs(message.CompletedJobs);
      }

      if (message.Finished) {
        toast({
          type: "success",
          title: "Job complete!",
          description: message.Finished.url,
          time: 5000,
        });
      }

      if (message.Failed) {
        console.error(message.Failed);
        toast({
          type: "error",
          title: "Job failed!",
          description: message.Failed.reason,
          time: 0,
        });
      }
    };

    socket.onclose = (msg) => {
      console.log(msg);
      if (!msg.wasClean) {
        console.log("unclean websocket shutdown");
        setConnected(false);
      }
    };

    socket.onerror = () => {
      setConnected(false);
    };

    socket.onopen = () => {
      setConnected(true);
    };

    return () => {
      socket.close(1000);
    };
  }, []);

  return (
    <div>
      <Container fluid style={{ padding: "1em 1em 0 1em" }}>
        <Header as="h2">
          Server Status: {connected ? `Connected` : `Disconnected`}
        </Header>
        <Grid columns={2} stackable>
          <Grid.Column>
            <Segment>
              <Header as="h3">Download Video</Header>
              <CreateJobForm />
            </Segment>
          </Grid.Column>
          <Grid.Column>
            <Header as="h3">Running jobs ({pendingJobs.length})</Header>
            <JobList jobs={pendingJobs} />
          </Grid.Column>
        </Grid>
        <Header as="h2">Completed Jobs</Header>
        <JobList jobs={completedJobs} />
        <SemanticToastContainer />
      </Container>
    </div>
  );
}

export default App;
