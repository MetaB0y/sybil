from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.event_traders_response import EventTradersResponse
from typing import cast



def _get_kwargs(
    event_id: str,

) -> dict[str, Any]:
    

    

    

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/events/{event_id}/traders".format(event_id=quote(str(event_id), safe=""),),
    }


    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> EventTradersResponse | None:
    if response.status_code == 200:
        response_200 = EventTradersResponse.from_dict(response.json())



        return response_200

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[EventTradersResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    event_id: str,
    *,
    client: AuthenticatedClient | Client,

) -> Response[EventTradersResponse]:
    """ GET /v1/events/{event_id}/traders

    Args:
        event_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[EventTradersResponse]
     """


    kwargs = _get_kwargs(
        event_id=event_id,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    event_id: str,
    *,
    client: AuthenticatedClient | Client,

) -> EventTradersResponse | None:
    """ GET /v1/events/{event_id}/traders

    Args:
        event_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        EventTradersResponse
     """


    return sync_detailed(
        event_id=event_id,
client=client,

    ).parsed

async def asyncio_detailed(
    event_id: str,
    *,
    client: AuthenticatedClient | Client,

) -> Response[EventTradersResponse]:
    """ GET /v1/events/{event_id}/traders

    Args:
        event_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[EventTradersResponse]
     """


    kwargs = _get_kwargs(
        event_id=event_id,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    event_id: str,
    *,
    client: AuthenticatedClient | Client,

) -> EventTradersResponse | None:
    """ GET /v1/events/{event_id}/traders

    Args:
        event_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        EventTradersResponse
     """


    return (await asyncio_detailed(
        event_id=event_id,
client=client,

    )).parsed
